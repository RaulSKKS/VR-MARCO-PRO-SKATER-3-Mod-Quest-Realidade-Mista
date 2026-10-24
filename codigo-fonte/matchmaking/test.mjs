// The Worker run against a real SQLite database standing in for D1 (Node 22's node:sqlite): rooms, guests, the
// address rules and the limits. Run: node matchmaking/test.mjs   (nothing is sent anywhere)
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { DatabaseSync } from "node:sqlite";
const src = fs.readFileSync(new URL("./worker.js", import.meta.url), "utf8").replace("export default", "const worker =") + "\nexport { worker, network, sameNetwork };";
const copy = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "skate-worker-test-")), "worker.mjs");
fs.writeFileSync(copy, src);
const { worker, network, sameNetwork } = await import(pathToFileURL(copy).href);

const sqlite = new DatabaseSync(":memory:");
sqlite.exec(fs.readFileSync(new URL("./schema.sql", import.meta.url), "utf8"));
// D1's prepare/bind/first/all/run over node:sqlite
const DB = { prepare(sql) { const st = sqlite.prepare(sql); let args = []; return {
    bind(...a) { args = a; return this; },
    async first() { return st.get(...args) ?? null; },
    async all() { return { results: st.all(...args) }; },
    async run() { const r = st.run(...args); return { meta: { changes: Number(r.changes) } }; },
}; } };

let clock = 1_800_000_000;
Date.now = () => clock * 1000;
const env = { DB }; // (no TURN key: no logins are asked of Cloudflare)
async function post(what, ip, body, e = env) {
    const r = await worker.fetch(new Request("https://x/v1/" + what, { method: "POST", headers: { "cf-connecting-ip": ip }, body: JSON.stringify(body) }), e);
    return r.json();
}
let fails = 0;
function check(name, ok) { console.log((ok ? "ok   " : "FAIL ") + name); if (!ok) fails++; }
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);

check("IPv6 network part", network("2001:db8:1:2:aaaa::1") === network("2001:db8:1:2::ffff") && network("2001:db8:1:3::1") !== network("2001:db8:1:2::1"));
check("sameNetwork: two IPv6 of one network", sameNetwork("2001:db8:1:2::10", [], "2001:db8:1:2:5::1", []));

// a room of three: host and two strangers
let h = await post("host", "198.51.100.1", { v: 1, name: "Marco", addrs: ["198.51.100.1:7000", "192.168.1.10:7000"], max: 3 });
check("host gets a 6-letter code and a key", /^[A-Z2-9]{6}$/.test(h.code) && h.key.length === 24 && h.you === "198.51.100.1");
let j1 = await post("join", "203.0.113.9", { v: 1, code: h.code.toLowerCase(), name: "Ana", addrs: ["203.0.113.9:8000", "10.0.0.5:8000"] });
check("a stranger joins (code in lower case): slot 1, host's local address hidden", j1.slot === 1 && same(j1.host.addrs, ["198.51.100.1:7000"]) && j1.host.name === "Marco");
let j2 = await post("join", "203.0.113.20", { v: 1, code: h.code, name: "Bia", addrs: ["203.0.113.20:9000"] });
check("a second stranger: slot 2", j2.slot === 2);
let j3 = await post("join", "203.0.113.30", { v: 1, code: h.code, name: "Caio", addrs: ["203.0.113.30:9000"] });
check("a third is refused: the room holds 3", j3.error === "full");
let p = await post("poll", "198.51.100.1", { code: h.code, key: h.key });
check("host's poll lists both guests with public and seen addresses only", same(p.members, [
    { slot: 1, name: "Ana", addrs: ["203.0.113.9:8000", "seen=203.0.113.9"] },
    { slot: 2, name: "Bia", addrs: ["203.0.113.20:9000", "seen=203.0.113.20"] }]));
await post("addrs", "198.51.100.1", { code: h.code, key: h.key, addrs: ["198.51.100.1:7000", "192.168.1.10:7000", "relay=104.30.1.1:5000"] });
p = await post("poll", "203.0.113.9", { code: h.code, key: j1.key });
check("a guest's poll shows the host's relay address, not its local one", same(p.host.addrs, ["198.51.100.1:7000", "relay=104.30.1.1:5000"]));
check("a guest may not change the host's addresses", (await post("addrs", "203.0.113.9", { code: h.code, key: j1.key, addrs: ["1.2.3.4:5"] })).error === "bad request");
check("a wrong key is no room", (await post("poll", "203.0.113.9", { code: h.code, key: "x".repeat(24) })).error === "no room");
await post("leave", "203.0.113.9", { code: h.code, key: j1.key });
p = await post("poll", "198.51.100.1", { code: h.code, key: h.key });
check("a guest who leaves frees its place", p.members.length === 1 && p.members[0].slot === 2);
j3 = await post("join", "203.0.113.30", { v: 1, code: h.code, name: "Caio", addrs: ["203.0.113.30:9000"] });
check("... and the next one gets slot 1", j3.slot === 1);
check("a different netcode version is refused", (await post("join", "203.0.113.40", { v: 2, code: h.code, name: "D", addrs: ["203.0.113.40:1"] })).error === "version");

// the room lives while the host polls
clock += 25;
await post("poll", "198.51.100.1", { code: h.code, key: h.key });
clock += 25;
check("polled 25 s ago: still there", (await post("poll", "203.0.113.20", { code: h.code, key: j2.key })).host !== undefined);
clock += 10;
check("35 s without the host's poll: gone", (await post("poll", "203.0.113.20", { code: h.code, key: j2.key })).error === "no room");
await post("host", "198.51.100.99", { v: 1, name: "X", addrs: ["198.51.100.99:1"] }); // (sweeps)
check("the guests went with it", sqlite.prepare("SELECT COUNT(*) AS n FROM members WHERE code = ?").get(h.code).n === 0);

// one household: local addresses handed over both ways
h = await post("host", "198.51.100.1", { v: 1, name: "H", addrs: ["198.51.100.1:7000", "192.168.1.10:7000"] });
j1 = await post("join", "198.51.100.1", { v: 1, code: h.code, name: "J", addrs: ["198.51.100.1:8000", "192.168.1.11:8000"] });
check("same address: host's local address handed over", same(j1.host.addrs, ["198.51.100.1:7000", "192.168.1.10:7000"]));
p = await post("poll", "198.51.100.1", { code: h.code, key: h.key });
check("host is told the housemate's local address", same(p.members[0].addrs, ["198.51.100.1:8000", "192.168.1.11:8000", "seen=198.51.100.1"]));
check("default room size is 4", sqlite.prepare("SELECT max FROM rooms WHERE code = ?").get(h.code).max === 4);
check("a room of 11 is refused", (await post("host", "198.51.100.5", { v: 1, name: "H", addrs: ["198.51.100.5:1"], max: 11 })).error === "bad request");
await post("leave", "198.51.100.1", { code: h.code, key: h.key });
check("the host's leave ends the room", (await post("poll", "198.51.100.1", { code: h.code, key: j1.key })).error === "no room");

// relay without a key
h = await post("host", "198.51.100.3", { v: 1, name: "H", addrs: ["198.51.100.3:7000"] });
check("relay before anyone joined: refused", (await post("relay", "198.51.100.3", { code: h.code, key: h.key })).error === "bad request");
await post("join", "203.0.113.50", { v: 1, code: h.code, name: "J", addrs: ["203.0.113.50:1"] });
let r = await post("relay", "198.51.100.3", { code: h.code, key: h.key });
check("no relay without a key says why", typeof r.noRelay === "string" && r.noRelay.includes("TURN_KEY_ID"));

// wrong codes
let last;
for (let i = 0; i < 31; i++) { last = await post("join", "192.0.2.77", { v: 1, code: "ZZZZZ" + "23456789ABCDEFGHJKMNPQRSTUVWXYZ"[i], name: "X", addrs: ["192.0.2.77:1"] }); }
check("the 31st wrong code in an hour is refused as busy", last.error === "busy");
last = await post("join", "192.0.2.77", { v: 1, code: h.code, name: "X", addrs: ["192.0.2.77:1"] });
check("... and so is a right code from that address", last.error === "busy");
// rooms an hour (20), with the room given up each time
for (let i = 0; i < 21; i++) { last = await post("host", "192.0.2.50", { v: 1, name: "H", addrs: ["192.0.2.50:1"] }); if (last.code) await post("leave", "192.0.2.50", { code: last.code, key: last.key }); }
check("the 21st room in an hour from one address is refused", last.error === "busy");
// open rooms per address (4)
for (let i = 0; i < 5; i++) { last = await post("host", "192.0.2.60", { v: 1, name: "H", addrs: ["192.0.2.60:1"] }); }
check("a 5th open room from one address is refused", last.error === "busy");
check("bad addresses are refused", (await post("host", "192.0.2.61", { v: 1, name: "H", addrs: ["not an address"] })).error === "bad request");

// MIN_RELEASE
const old = { DB, MIN_RELEASE: "3" };
check("hello says the minimum", (await post("hello", "192.0.2.70", { rel: 1 }, old)).min === 3);
check("an old release may not host", (await post("host", "192.0.2.70", { v: 1, rel: 2, name: "H", addrs: ["192.0.2.70:1"] }, old)).error === "old");
check("a new one may", typeof (await post("host", "192.0.2.70", { v: 1, rel: 3, name: "H", addrs: ["192.0.2.70:1"] }, old)).code === "string");

// the relay with a key and its use measured: Cloudflare's analytics and TURN service stood in for
const cap = { DB, TURN_KEY_ID: "abc123", TURN_KEY_TOKEN: "t", CF_ACCOUNT_ID: "0123abcd", CF_ANALYTICS_TOKEN: "a", ADMIN_KEY: "0123456789abcdef0123" };
let monthBytes = 10e9;
const revoked = [];
globalThis.fetch = async (u) => {
    u = String(u);
    if (u.includes("/graphql")) {
        return new Response(JSON.stringify({ data: { viewer: { accounts: [{ month: [{ sum: { egressBytes: monthBytes / 2, ingressBytes: monthBytes / 2 }, dimensions: { keyId: "abc123" } }],
            logins: [{ sum: { egressBytes: 2.5e9, ingressBytes: 0.5e9 }, dimensions: { username: "heavy" } }, { sum: { egressBytes: 5e7, ingressBytes: 5e7 }, dimensions: { username: "light" } }] }] } } }));
    }
    if (u.endsWith("/revoke")) { revoked.push(u.split("/credentials/")[1].split("/")[0]); return new Response(null, { status: 204 }); }
    if (u.includes("generate-ice-servers")) { return new Response(JSON.stringify({ iceServers: [{ urls: ["stun:stun.example:3478", "turn:turn.example:3478?transport=udp"], username: "u", credential: "c" }] })); }
    throw new Error("unexpected fetch " + u);
};
h = await post("host", "192.0.2.90", { v: 1, name: "H", addrs: ["192.0.2.90:1"] }, cap);
await post("join", "203.0.113.90", { v: 1, code: h.code, name: "J", addrs: ["203.0.113.90:1"] }, cap);
r = await post("relay", "192.0.2.90", { code: h.code, key: h.key }, cap);
check("measuring set up but never done: no relay login", typeof r.noRelay === "string" && r.noRelay.includes("measured"));
check("status needs the admin key", (await post("status", "192.0.2.90", { admin: "wrong" }, cap)).error === "bad request");
let st = await post("status", "192.0.2.90", { admin: cap.ADMIN_KEY }, cap);
check("under the limit: relay on, 10 GB counted", st.measured === true && st.relay === "on" && st.monthGB === 10);
check("the login that passed 3 GB is ended, the light one is not", same(revoked, ["heavy"]));
r = await post("relay", "192.0.2.90", { code: h.code, key: h.key }, cap);
check("after a measurement: a UDP relay login is given", r.turn && r.turn.server === "turn.example:3478" && r.turn.username === "u");
monthBytes = 600e9;
st = await post("status", "192.0.2.90", { admin: cap.ADMIN_KEY }, cap);
check("past the month's limit: relay off, every login in use ended", st.relay === "off until next month" && revoked.includes("light"));
r = await post("relay", "192.0.2.90", { code: h.code, key: h.key }, cap);
check("... and no more logins", r.noRelay === "the month's relay allowance is used up");
await worker.scheduled({}, cap, { waitUntil: (x) => x });
check("the Cron Trigger's entry runs", true);

console.log(fails === 0 ? "\nall passed" : `\n${fails} failed`);
process.exit(fails === 0 ? 0 : 1);
