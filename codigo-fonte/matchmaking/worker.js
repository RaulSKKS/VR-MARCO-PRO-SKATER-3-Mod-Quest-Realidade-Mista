// Matchmaking for the Skate 3 port's online free-skate: room codes.
// SPDX-License-Identifier: AGPL-3.0-only (adapted from Tenkaichi3Decomp's Worker, which is AGPL-3.0).
//
// Adapted from Tenkaichi3Decomp's matchmaking Worker (port/matchmaking), for rooms of several players: the host makes
// a room and gets a short code, up to `max - 1` others join with it, and each guest connects to the host (the game's
// sessions are a star: the host forwards everyone's packets). No game data passes through here.
//
// What it stores (a D1 database bound as DB, see schema.sql): per room the code, the host's name, addresses and
// address as Cloudflare saw it; per guest its name and addresses. A room lasts while its host keeps polling (every
// two seconds): ROOM_IDLE seconds after the host's last poll it is gone, and in any case ROOM_MAX after it was made.
//
// Requests are JSON over POST:
//   /v1/host   {v, rel, name, addrs, max}   -> {code, key, you}
//   /v1/join   {v, rel, code, name, addrs}  -> {key, slot, you, host: {name, addrs}}
//   /v1/poll   {code, key}  the host        -> {members: [{slot, name, addrs}]}   (also keeps the room alive)
//              {code, key}  a guest         -> {host: {name, addrs}}             (the host's relay address shows up here)
//   /v1/addrs  {code, key, addrs}           -> {}   (the host, once it has more addresses: its relay address)
//   /v1/relay  {code, key}                  -> {turn} | {noRelay}   (the host, once a guest could not be reached
//       directly: a relay login, asked for only then)
//   /v1/leave  {code, key}                  -> {}   (the host ends the room; a guest gives up its place)
//   /v1/hello  {rel}                        -> {min, latest, now}
// `turn` (only if the Worker has a TURN key: the secrets TURN_KEY_ID and TURN_KEY_TOKEN) is {server, username,
// credential}, good for three hours, for Cloudflare's TURN service. The host publishes its relay address as
// "relay=ip:port"; guests' addresses carry "seen=ip", the address this Worker saw them under (the relay must be told
// whom to let through). `v` is the game's netcode version (host and guests must agree), `addrs` a list of "ip:port"
// strings, `you` the caller's address as seen here. Errors come back as {error: "..."} with a 4xx status:
// "version", "no room", "full", "busy", "bad request", "old" (below MIN_RELEASE).

const ROOM_IDLE = 30;            // seconds a room outlives its host's last poll
const ROOM_MAX = 6 * 3600;       // seconds a room lasts at most
const MAX_PLAYERS = 10;          // the game's own ceiling (skate-net lobby::MAX_PLAYERS)
const DEFAULT_PLAYERS = 4;
const ROOMS_PER_ADDRESS = 4;     // rooms one address may have open at a time
const WRONG_CODES_PER_HOUR = 30; // codes that name no room, from one address in an hour
const HOSTS_PER_HOUR = 20;       // rooms one address may make in an hour
const RELAY_PER_DAY_ADDRESS = 10; // relay logins one address may be given in a day
const RELAY_PER_DAY = 500;       // relay logins given in a day, all told (the variable TURN_DAILY_MAX overrides it)
// The relay's use is measured (see relayCheck) when the Worker is given the means: the variable CF_ACCOUNT_ID and the
// secret CF_ANALYTICS_TOKEN (an API token with "Account Analytics: Read"), and a Cron Trigger every five minutes.
const RELAY_MONTH_GB = 500;      // bytes through the relay in a calendar month after which no more logins are given
                                 // and those in use are ended (TURN_MONTHLY_GB overrides it). Cloudflare charges after 1,000 GB.
const RELAY_LOGIN_GB = 2;        // one login that has passed this much in three hours is ended (a host with nine
                                 // guests on the relay sends about 0.3 GB an hour; TURN_LOGIN_GB overrides it)
const CHECK_STALE = 2 * 3600;    // with measuring set up: no logins when no measurement has succeeded for this long
const ALPHABET = "ABCDEFGHJKMNPQRSTUVWXYZ23456789"; // no I, L, O, 0, 1: they are misread

function reply(body, status = 200) {
    return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", "cache-control": "no-store" } });
}

function randomText(length, alphabet) {
    const bytes = new Uint8Array(length);
    crypto.getRandomValues(bytes);
    let out = "";
    for (const b of bytes) {
        out += alphabet[b % alphabet.length];
    }
    return out;
}

// A name as the game sends it: printable ASCII, at most 16 characters.
function cleanName(name) {
    return String(name ?? "Skater").replace(/[^\x20-\x7e]/g, "").slice(0, 16) || "Skater";
}

// The addresses a game reports: up to five "ip:port" strings.
function cleanAddrs(addrs) {
    if (!Array.isArray(addrs)) {
        return null;
    }
    const out = [];
    for (const a of addrs.slice(0, 5)) {
        if (typeof a === "string" && /^(relay=)?[0-9a-fA-F:.\[\]]{3,64}:\d{1,5}$/.test(a)) {
            out.push(a);
        }
    }
    return out.length > 0 ? out : null;
}

// An address of a private network ("192.168.1.20:7000"): of use only to someone on that network.
function isPrivate(a) {
    const m = /^(\d+)\.(\d+)\./.exec(a);
    if (!m) {
        return false;
    }
    const x = Number(m[1]), y = Number(m[2]);
    return x === 10 || x === 127 || (x === 172 && y >= 16 && y <= 31) || (x === 192 && y === 168) || (x === 169 && y === 254);
}

// What tells two machines of one household apart from strangers: the IPv4 address as it is, of an IPv6 address
// the first four groups (the network's part).
function network(ip) {
    if (!ip.includes(":")) {
        return ip;
    }
    const [head, tail] = ip.split("::");
    const h = head ? head.split(":") : [], t = tail ? tail.split(":") : [];
    const all = tail === undefined ? h : [...h, ...Array(Math.max(0, 8 - h.length - t.length)).fill("0"), ...t];
    return all.slice(0, 4).map((g) => parseInt(g, 16) || 0).join(":");
}

// The public IPv4 addresses among what a game said about itself.
function publicIps(addrs) {
    const out = [];
    for (const a of addrs) {
        const m = /^(\d+\.\d+\.\d+\.\d+):/.exec(a);
        if (m && !isPrivate(a)) {
            out.push(m[1]);
        }
    }
    return out;
}

// Whether the two are on one network: seen by this Worker under the same address (or IPv6 network), or, when one
// came by IPv4 and the other by IPv6, with the same public IPv4 address in what they found out themselves.
function sameNetwork(ipA, addrsA, ipB, addrsB) {
    if (ipA !== "" && network(ipA) === network(ipB)) {
        return true;
    }
    const b = publicIps(addrsB);
    return publicIps(addrsA).some((x) => b.includes(x));
}

// A player's addresses as another gets them: those of a private network only for someone on it.
function shown(addrs, same) {
    return addrs.filter((a) => same || !isPrivate(a));
}

// How many uses of `what` the slot has so far (0 if none or not known).
async function usesSoFar(env, what, slot) {
    try {
        const row = await env.DB.prepare("SELECT n FROM used WHERE what = ? AND slot = ?").bind(what, slot).first();
        return row ? row.n : 0;
    } catch (e) {
        return 0;
    }
}

// Counts one more use of `what` in the time slot that starts at `slot` (seconds) and says how many that makes; 0 if
// the count could not be kept (the table "used" is missing: see schema.sql).
async function countUse(env, what, slot) {
    try {
        const row = await env.DB.prepare("INSERT INTO used (what, slot, n) VALUES (?, ?, 1) ON CONFLICT(what, slot) DO UPDATE SET n = n + 1 RETURNING n")
            .bind(what, slot).first();
        return row ? row.n : 0;
    } catch (e) {
        return 0;
    }
}

// A kept number: the value of `what` in `slot` (null if there is none), and setting it.
async function getValue(env, what, slot) {
    try {
        const row = await env.DB.prepare("SELECT n FROM used WHERE what = ? AND slot = ?").bind(what, slot).first();
        return row ? row.n : null;
    } catch (e) {
        return null;
    }
}

async function setValue(env, what, slot, n) {
    await env.DB.prepare("INSERT INTO used (what, slot, n) VALUES (?, ?, ?) ON CONFLICT(what, slot) DO UPDATE SET n = excluded.n").bind(what, slot, n).run();
}

function monthStart(now) {
    const d = new Date(now * 1000);
    return Math.floor(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1) / 1000);
}

function measuring(env) {
    return Boolean(env.CF_ACCOUNT_ID && env.CF_ANALYTICS_TOKEN && env.TURN_KEY_ID && env.TURN_KEY_TOKEN);
}

// Ends one relay login at once.
async function revoke(env, username) {
    try {
        const r = await fetch(`https://rtc.live.cloudflare.com/v1/turn/keys/${env.TURN_KEY_ID}/credentials/${encodeURIComponent(username)}/revoke`, {
            method: "POST",
            headers: { authorization: `Bearer ${env.TURN_KEY_TOKEN}` },
        });
        return r.ok;
    } catch (e) {
        return false;
    }
}

// Measures what has gone through the relay (Cloudflare's analytics, minutes behind) and acts on it: a login that has
// passed more than a session ever would is ended; past the month's limit every login in use is ended and no more are
// given until the next month. Run by the Cron Trigger, and by /v1/status. Returns what it found.
async function relayCheck(env, now) {
    if (!measuring(env)) {
        return { measured: false, why: "CF_ACCOUNT_ID and CF_ANALYTICS_TOKEN are not both set" };
    }
    const account = String(env.CF_ACCOUNT_ID).replace(/[^0-9a-f]/gi, ""), keyId = String(env.TURN_KEY_ID).replace(/[^0-9a-z]/gi, "");
    const month = monthStart(now), iso = (t) => new Date(t * 1000).toISOString().slice(0, 19) + "Z";
    const part = (name, from, extra) => `${name}: callsTurnUsageAdaptiveGroups(filter: { datetime_geq: "${iso(from)}", datetime_leq: "${iso(now)}", keyId: "${keyId}" }, ${extra}) { sum { egressBytes ingressBytes } dimensions { ${name === "logins" ? "username" : "keyId"} } }`;
    const query = `query { viewer { accounts(filter: { accountTag: "${account}" }) { ${part("month", month, "limit: 10")} ${part("logins", now - 3 * 3600, "limit: 500, orderBy: [sum_egressBytes_DESC]")} } } }`;
    let data;
    try {
        const r = await fetch("https://api.cloudflare.com/client/v4/graphql", {
            method: "POST",
            headers: { authorization: `Bearer ${env.CF_ANALYTICS_TOKEN}`, "content-type": "application/json" },
            body: JSON.stringify({ query }),
        });
        const j = await r.json();
        if (!r.ok || (j.errors && j.errors.length) || !j.data) {
            return { measured: false, why: "the analytics answered " + r.status + ": " + JSON.stringify(j.errors ?? j).slice(0, 300) };
        }
        data = j.data.viewer.accounts[0];
    } catch (e) {
        return { measured: false, why: "the analytics could not be asked: " + String(e).slice(0, 200) };
    }
    if (!data) {
        return { measured: false, why: "the analytics know no such account (CF_ACCOUNT_ID)" };
    }
    // both directions are counted: more than Cloudflare may charge for, so the limit is reached early rather than late
    const bytes = (g) => (g.sum.egressBytes ?? 0) + (g.sum.ingressBytes ?? 0);
    const monthBytes = (data.month ?? []).reduce((n, g) => n + bytes(g), 0);
    const monthLimit = (Number(env.TURN_MONTHLY_GB) || RELAY_MONTH_GB) * 1e9, loginLimit = (Number(env.TURN_LOGIN_GB) || RELAY_LOGIN_GB) * 1e9;
    const over = monthBytes >= monthLimit;
    let ended = 0;
    for (const g of data.logins ?? []) {
        const name = g.dimensions.username;
        if (name && (over || bytes(g) >= loginLimit) && (await getValue(env, "ended:" + name, 0)) === null) {
            if (await revoke(env, name)) {
                await setValue(env, "ended:" + name, 0, now);
                ended++;
            }
        }
    }
    if (over) {
        await setValue(env, "relayoff", month, 1);
    }
    await setValue(env, "checked", 0, now);
    return { measured: true, monthGB: Math.round(monthBytes / 1e7) / 100, limitGB: monthLimit / 1e9, relay: over ? "off until next month" : "on", loginsSeen: (data.logins ?? []).length, loginsEnded: ended };
}

// Whether this address may be given a relay login now (and counts it), or why not.
async function relayAllowed(env, ip, now) {
    if ((await getValue(env, "relayoff", monthStart(now))) !== null) {
        return "the month's relay allowance is used up";
    }
    if (env.CF_ACCOUNT_ID || env.CF_ANALYTICS_TOKEN) {
        const checked = await getValue(env, "checked", 0);
        if (checked === null || now - checked > CHECK_STALE) {
            return "the relay's use could not be measured lately: no logins until it can (see /v1/status)";
        }
    }
    const day = now - (now % 86400);
    const all = await countUse(env, "relay", day);
    if (all === 0) {
        return "the Worker cannot count relay logins (the table 'used' is missing): none are given";
    }
    if (all > (Number(env.TURN_DAILY_MAX) || RELAY_PER_DAY)) {
        return "the day's relay logins are used up";
    }
    if ((await countUse(env, "relay:" + ip, day)) > RELAY_PER_DAY_ADDRESS) {
        return "too many relay logins for this address today";
    }
    return null;
}

// Short-lived access to Cloudflare's TURN service for one host: {server, username, credential}, or {why} saying what
// stood in the way (never anything secret: for whoever sets the Worker up).
async function turnAccess(env) {
    if (!env.TURN_KEY_ID || !env.TURN_KEY_TOKEN) {
        return { why: `no key: the secret ${!env.TURN_KEY_ID ? "TURN_KEY_ID" : "TURN_KEY_TOKEN"} is not set` };
    }
    try {
        const r = await fetch(`https://rtc.live.cloudflare.com/v1/turn/keys/${env.TURN_KEY_ID}/credentials/generate-ice-servers`, {
            method: "POST",
            headers: { authorization: `Bearer ${env.TURN_KEY_TOKEN}`, "content-type": "application/json" },
            body: JSON.stringify({ ttl: 10800 }),
        });
        if (!r.ok) {
            return { why: `the TURN service answered ${r.status} (401 or 403: the key ID or the token is not right)` };
        }
        const j = await r.json();
        // iceServers is a list of entries, or (older answers) one entry
        const list = Array.isArray(j.iceServers) ? j.iceServers : j.iceServers ? [j.iceServers] : [];
        for (const s of list) {
            const urls = Array.isArray(s.urls) ? s.urls : [s.urls];
            const udp = urls.find((u) => typeof u === "string" && u.startsWith("turn:") && u.includes("transport=udp"));
            if (udp && s.username && s.credential) {
                return { server: udp.slice(5).split("?")[0], username: s.username, credential: s.credential };
            }
        }
        return { why: "the TURN service's answer had no UDP relay in it" };
    } catch (e) {
        return { why: "the TURN service could not be asked" };
    }
}

// Rooms whose host stopped polling, or that are too old, are gone (their guests with them).
async function sweep(env, now) {
    await env.DB.prepare("DELETE FROM members WHERE code IN (SELECT code FROM rooms WHERE alive < ? OR made < ?)").bind(now - ROOM_IDLE, now - ROOM_MAX).run();
    await env.DB.prepare("DELETE FROM rooms WHERE alive < ? OR made < ?").bind(now - ROOM_IDLE, now - ROOM_MAX).run();
}

// Puts a guest into a room: {key, slot, you, host} or null if the room is full.
async function joinRoom(env, room, ip, addrs, name, now) {
    const key = randomText(24, ALPHABET);
    const hostAddrs = JSON.parse(room.host_addrs);
    const same = sameNetwork(ip, addrs, room.ip, hostAddrs);
    // what the host will be told of this guest (kept as it will be shown)
    const kept = shown(addrs, same);
    if (/^[0-9.]{7,15}$/.test(ip)) {
        kept.push("seen=" + ip); // for the host's relay: whom to let through
    }
    for (let attempt = 0; attempt < 3; attempt++) {
        const taken = await env.DB.prepare("SELECT slot FROM members WHERE code = ?").bind(room.code).all();
        const used = new Set(((taken && taken.results) || []).map((r) => r.slot));
        let slot = 1;
        while (used.has(slot)) {
            slot++;
        }
        if (slot >= room.max) {
            return null;
        }
        try {
            await env.DB.prepare("INSERT INTO members (code, slot, key, name, addrs, ip, same, joined) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(room.code, slot, key, cleanName(name), JSON.stringify(kept), ip, same ? 1 : 0, now).run();
            return { key, slot, you: ip, host: { name: room.host_name, addrs: shown(hostAddrs, same) } };
        } catch (e) {
            // another guest took that slot in the same moment: the next one
        }
    }
    return null;
}

export default {
    async fetch(request, env) {
        const url = new URL(request.url);
        if (request.method === "GET" && url.pathname === "/") {
            return new Response("Skate 3 port matchmaking: room codes for online free-skate. See the game's repository, matchmaking/.\n");
        }
        if (request.method !== "POST" || !url.pathname.startsWith("/v1/")) {
            return reply({ error: "bad request" }, 404);
        }
        let body;
        try {
            body = JSON.parse(await request.text());
        } catch (e) {
            return reply({ error: "bad request" }, 400);
        }
        if (body === null || typeof body !== "object") {
            return reply({ error: "bad request" }, 400);
        }
        const ip = request.headers.get("cf-connecting-ip") ?? "";
        const now = Math.floor(Date.now() / 1000);
        const what = url.pathname.slice(4);

        if (what === "status") { // for whoever runs the Worker: measures now and says what it found (no secrets in it)
            if (!env.ADMIN_KEY || String(env.ADMIN_KEY).length < 16 || body.admin !== env.ADMIN_KEY) {
                return reply({ error: "bad request" }, 404);
            }
            const day = now - (now % 86400);
            const open = await env.DB.prepare("SELECT COUNT(*) AS n FROM rooms WHERE alive >= ?").bind(now - ROOM_IDLE).first();
            return reply({ ...(await relayCheck(env, now)), loginsToday: (await getValue(env, "relay", day)) ?? 0, lastMeasured: await getValue(env, "checked", 0), rooms: open ? open.n : 0 });
        }
        // Only the newest release, if the Worker's keeper says so: the variable MIN_RELEASE refuses a new room or a
        // join from a game that says it is older, or does not say. Rooms under way are not touched.
        const minRelease = Number(env.MIN_RELEASE ?? 0) || 0;
        if (what === "hello") {
            return reply({ min: minRelease, latest: Number(env.LATEST_RELEASE ?? 0) || 0, now });
        }
        if (minRelease > 0 && (what === "host" || what === "join") && !(Number.isInteger(body.rel) && body.rel >= minRelease)) {
            return reply({ error: "old", need: minRelease }, 426);
        }
        if (what === "host") {
            const addrs = cleanAddrs(body.addrs);
            const max = body.max === undefined ? DEFAULT_PLAYERS : body.max;
            if (!Number.isInteger(body.v) || addrs === null || !Number.isInteger(max) || max < 2 || max > MAX_PLAYERS) {
                return reply({ error: "bad request" }, 400);
            }
            await sweep(env, now);
            const open = await env.DB.prepare("SELECT COUNT(*) AS n FROM rooms WHERE ip = ?").bind(ip).first();
            if (open && open.n >= ROOMS_PER_ADDRESS) {
                return reply({ error: "busy" }, 429);
            }
            if ((await countUse(env, "host:" + ip, now - (now % 3600))) > HOSTS_PER_HOUR) {
                return reply({ error: "busy" }, 429);
            }
            if (now % 50 === 0) { // now and then: counts of past days are thrown away
                await env.DB.prepare("DELETE FROM used WHERE slot < ? AND slot > 0 AND what != 'relayoff'").bind(now - 2 * 86400).run().catch(() => {});
                await env.DB.prepare("DELETE FROM used WHERE slot = 0 AND what LIKE 'ended:%' AND n < ?").bind(now - 2 * 86400).run().catch(() => {});
            }
            const key = randomText(24, ALPHABET);
            for (let attempt = 0; attempt < 5; attempt++) {
                const code = randomText(6, ALPHABET);
                try {
                    await env.DB.prepare("INSERT INTO rooms (code, made, alive, version, max, ip, host_key, host_name, host_addrs) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
                        .bind(code, now, now, body.v, max, ip, key, cleanName(body.name), JSON.stringify(addrs)).run();
                    return reply({ code, key, you: ip });
                } catch (e) {
                    // the code is taken: another one
                }
            }
            return reply({ error: "busy" }, 503);
        }

        const code = String(body.code ?? "").toUpperCase().replace(/[^A-Z0-9]/g, "");
        if (code.length !== 6) {
            return reply({ error: "no room" }, 404);
        }
        const hour = now - (now % 3600);
        if (what === "join" && (await usesSoFar(env, "miss:" + ip, hour)) >= WRONG_CODES_PER_HOUR) {
            return reply({ error: "busy" }, 429); // (codes are not to be found by trying)
        }
        const room = await env.DB.prepare("SELECT * FROM rooms WHERE code = ? AND alive >= ? AND made >= ?").bind(code, now - ROOM_IDLE, now - ROOM_MAX).first();
        if (!room) {
            if (what === "join") {
                await countUse(env, "miss:" + ip, hour);
            }
            return reply({ error: "no room" }, 404);
        }

        if (what === "join") {
            const addrs = cleanAddrs(body.addrs);
            if (!Number.isInteger(body.v) || addrs === null) {
                return reply({ error: "bad request" }, 400);
            }
            if (body.v !== room.version) {
                return reply({ error: "version" }, 409);
            }
            const joined = await joinRoom(env, room, ip, addrs, body.name, now);
            return joined ? reply(joined) : reply({ error: "full" }, 409);
        }

        const key = String(body.key ?? "");
        const isHost = key === room.host_key;
        const member = isHost ? null : await env.DB.prepare("SELECT * FROM members WHERE code = ? AND key = ?").bind(code, key).first();
        if (!isHost && !member) {
            return reply({ error: "no room" }, 404);
        }
        if (what === "poll") {
            if (isHost) {
                await env.DB.prepare("UPDATE rooms SET alive = ? WHERE code = ?").bind(now, code).run();
                const rows = await env.DB.prepare("SELECT slot, name, addrs FROM members WHERE code = ? ORDER BY slot").bind(code).all();
                return reply({ members: ((rows && rows.results) || []).map((r) => ({ slot: r.slot, name: r.name, addrs: JSON.parse(r.addrs) })) });
            }
            return reply({ host: { name: room.host_name, addrs: shown(JSON.parse(room.host_addrs), member.same === 1) } });
        }
        if (what === "relay") { // the host: a guest could not be reached directly, a relay login is needed after all
            const anyone = await env.DB.prepare("SELECT COUNT(*) AS n FROM members WHERE code = ?").bind(code).first();
            if (!isHost || !anyone || anyone.n === 0) {
                return reply({ error: "bad request" }, 400); // (only once someone has joined: nobody waits with a login)
            }
            const refused = await relayAllowed(env, ip, now);
            const turn = refused ? { why: refused } : await turnAccess(env);
            return reply(turn.server ? { turn } : { noRelay: turn.why });
        }
        if (what === "addrs") { // the host has found out more about how it can be reached
            const addrs = cleanAddrs(body.addrs);
            if (!isHost || addrs === null) {
                return reply({ error: "bad request" }, 400);
            }
            await env.DB.prepare("UPDATE rooms SET host_addrs = ? WHERE code = ?").bind(JSON.stringify(addrs), code).run();
            return reply({});
        }
        if (what === "leave") {
            if (isHost) {
                await env.DB.prepare("DELETE FROM members WHERE code = ?").bind(code).run();
                await env.DB.prepare("DELETE FROM rooms WHERE code = ?").bind(code).run();
            } else {
                await env.DB.prepare("DELETE FROM members WHERE code = ? AND key = ?").bind(code, key).run();
            }
            return reply({});
        }
        return reply({ error: "bad request" }, 404);
    },

    // the Cron Trigger: the relay's use is measured
    async scheduled(event, env, ctx) {
        ctx.waitUntil(relayCheck(env, Math.floor(Date.now() / 1000)));
    },
};
