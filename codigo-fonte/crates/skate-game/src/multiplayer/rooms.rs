//! Online rooms: a transport that finds its peers through the matchmaking Worker (a 6-letter code), reaches them
//! through their NATs by sending from both sides at once, and falls back to the host's TURN relay. One UDP socket
//! carries everything (STUN, TURN, game). Peer ids are virtual and fixed — the host is `HOST`, the guest in slot n
//! is `GUEST + n` — so the session never notices a guest moving from the relay to a direct path or back.
use super::transport::Transport;
use skate_net::{
    rooms::{self, Answer, Client},
    stun,
    turn::Turn,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io,
    net::{IpAddr, SocketAddr, UdpSocket},
    sync::Mutex,
    time::{Duration, Instant},
};

pub(super) const HOST: u64 = 1;
const GUEST: u64 = 1000;
/// What both ends send each other's addresses while no game packet has come through (consumed here, never game data).
const PUNCH: &[u8] = b"SK8PUNCH";
const POLL_EVERY: Duration = Duration::from_secs(3);
const POLL_SETTLED: Duration = Duration::from_secs(15);
const PUNCH_EVERY: Duration = Duration::from_millis(200);
const PUNCH_SLOW: Duration = Duration::from_secs(1);
const STUN_RETRY: Duration = Duration::from_millis(500);
/// A guest the host has not heard from this long after it joined: the relay is asked for.
const RELAY_AFTER: Duration = Duration::from_secs(4);
/// A guest tries the host's relay address only this long after joining (a direct path is better when there is one).
const RELAY_DELAY: Duration = Duration::from_secs(3);
/// A path not heard from this long is no longer preferred.
const PATH_STALE: Duration = Duration::from_secs(3);
const GIVE_UP: Duration = Duration::from_secs(30);

pub(super) enum Start {
    Host { max: u32 },
    Join(String),
}

#[derive(Clone, Copy)]
struct Path {
    addr: SocketAddr,
    relay: bool,
    heard: Instant,
}
impl Path {
    /// Whether a packet that came by `relay` should become the path: a direct one always, the relay only when
    /// nothing direct was heard lately.
    fn better(current: Option<Path>, relay: bool) -> bool {
        current.is_none_or(|p| !relay || p.relay || p.heard.elapsed() > PATH_STALE)
    }
}
struct Guest {
    name: String,
    addrs: Vec<SocketAddr>,
    ips: Vec<IpAddr>,
    appeared: Instant,
    path: Option<Path>,
}

#[derive(PartialEq)]
enum Phase {
    /// Waiting for the STUN servers' addresses.
    Resolving,
    /// Asking them what this socket looks like from outside.
    Stun,
    /// The Worker's host/join answer is awaited.
    Registering,
    Ready,
    Failed,
}

pub(super) struct Rooms {
    socket: UdpSocket,
    client: Mutex<Client>,
    hosting: bool,
    name: String,
    max: u32,
    code: String,
    key: String,
    phase: Phase,
    began: Instant,
    stun_servers: Vec<SocketAddr>,
    stun_transaction: [u8; 12],
    stun_tries: u32,
    stun_sent: Instant,
    public: Option<SocketAddr>,
    lan: Option<SocketAddr>,
    last_poll: Instant,
    poll_waiting: bool,
    last_punch: Instant,
    status: String,
    // the host's side
    guests: BTreeMap<u32, Guest>,
    turn: Option<Turn>,
    relay_asked: bool,
    relay_published: bool,
    relay_note: String,
    pending_login: Option<(String, String)>,
    // a guest's side
    candidates: Vec<(SocketAddr, bool)>,
    joined: Option<Instant>,
    host_path: Option<Path>,
}

fn parse_addrs(list: &Value) -> (Vec<(SocketAddr, bool)>, Vec<IpAddr>) {
    let mut addrs = vec![];
    let mut seen = vec![];
    for a in list.as_array().into_iter().flatten().filter_map(Value::as_str) {
        if let Some(ip) = a.strip_prefix("seen=") {
            seen.extend(ip.parse::<IpAddr>().ok());
        } else if let Some(r) = a.strip_prefix("relay=") {
            addrs.extend(r.parse::<SocketAddr>().ok().map(|a| (a, true)));
        } else {
            addrs.extend(a.parse::<SocketAddr>().ok().map(|a| (a, false)));
        }
    }
    (addrs, seen)
}

/// This machine's address on its own network: the interface a packet to the internet would leave by.
fn lan_ip() -> Option<IpAddr> {
    let probe = UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect("192.0.2.1:9").ok()?;
    let ip = probe.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

/// Where the matchmaking Worker answers: --net-match-url, SK8_MATCH_URL at run time, SK8_MATCH_URL when this build
/// was made (how the Quest gets it), or the built-in default.
pub(super) fn match_url(option: Option<&str>) -> String {
    option
        .map(str::to_owned)
        .or_else(|| std::env::var("SK8_MATCH_URL").ok())
        .or_else(|| option_env!("SK8_MATCH_URL").map(str::to_owned))
        .unwrap_or_else(|| rooms::DEFAULT_URL.to_owned())
        .trim()
        .to_owned()
}

impl Rooms {
    pub fn new(url: String, start: Start, name: &str) -> Result<Self, String> {
        if url.is_empty() {
            return Err("Online rooms are not set up: no matchmaking URL (see matchmaking/README.md)".into());
        }
        let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
        skate_net::socket::configure(&socket).map_err(|e| e.to_string())?;
        let port = socket.local_addr().map_err(|e| e.to_string())?.port();
        let client = Client::new(url);
        client.resolve("stun", rooms::STUN_SERVERS.iter().map(|s| s.to_string()).collect());
        let (hosting, max, code) = match start {
            Start::Host { max } => (true, max.clamp(2, skate_net::lobby::MAX_PLAYERS as u32), String::new()),
            Start::Join(code) => {
                let code = rooms::clean_code(&code);
                if code.len() != 6 {
                    return Err("A room code has 6 letters and digits".into());
                }
                (false, 0, code)
            }
        };
        let seed = super::unique();
        let mut transaction = [0; 12];
        transaction[..8].copy_from_slice(&seed.to_le_bytes());
        let now = Instant::now();
        Ok(Self {
            socket,
            client: Mutex::new(client),
            hosting,
            name: name.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(16).collect(),
            max,
            code,
            key: String::new(),
            phase: Phase::Resolving,
            began: now,
            stun_servers: vec![],
            stun_transaction: transaction,
            stun_tries: 0,
            stun_sent: now,
            public: None,
            lan: lan_ip().map(|ip| SocketAddr::new(ip, port)),
            last_poll: now,
            poll_waiting: false,
            last_punch: now,
            status: "Finding this connection's address...".into(),
            guests: BTreeMap::new(),
            turn: None,
            relay_asked: false,
            relay_published: false,
            relay_note: String::new(),
            pending_login: None,
            candidates: vec![],
            joined: None,
            host_path: None,
        })
    }
    fn post(&self, path: &'static str, body: Value) {
        self.client.lock().unwrap().post(path, body);
    }
    fn own_addrs(&self) -> Vec<String> {
        let mut out: Vec<String> = self.public.iter().chain(&self.lan).map(|a| a.to_string()).collect();
        if let Some(relay) = self.turn.as_ref().and_then(Turn::relayed) {
            out.push(format!("relay={relay}"));
        }
        out.dedup();
        out
    }
    fn fail(&mut self, why: String) {
        self.phase = Phase::Failed;
        self.status = why;
    }
    fn register(&mut self) {
        let addrs = self.own_addrs();
        if addrs.is_empty() {
            return self.fail("No network: this device has no address to share".into());
        }
        self.phase = Phase::Registering;
        if self.hosting {
            self.status = "Making a room...".into();
            self.post("host", rooms::host_body(&self.name, &addrs, self.max));
        } else {
            self.status = format!("Joining room {}...", self.code);
            self.post("join", rooms::join_body(&self.code, &self.name, &addrs));
        }
    }
    fn answers(&mut self) {
        let answers = self.client.lock().unwrap().answers();
        for answer in answers {
            match answer {
                Answer::Resolved("stun", addrs) => {
                    self.stun_servers = addrs;
                    self.phase = Phase::Stun;
                }
                Answer::Resolved(_, addrs) => self.start_relay(addrs.first().copied()),
                Answer::Reply(path, reply) => {
                    if path == "poll" {
                        self.poll_waiting = false;
                    }
                    match reply {
                        Ok(v) if v.get("error").is_some() => {
                            let error = v["error"].as_str().unwrap_or("?").to_owned();
                            self.refused(path, &error);
                        }
                        Ok(v) => self.answered(path, v),
                        Err(e) if path == "host" || path == "join" => {
                            self.fail(format!("The matchmaking service could not be reached: {e}"))
                        }
                        Err(_) => {} // (a poll or a leave lost: the next one goes in a few seconds)
                    }
                }
            }
        }
    }
    fn refused(&mut self, path: &str, error: &str) {
        match path {
            "host" | "join" => self.fail(rooms::explain(error)),
            "poll" if error == "no room" && !self.connected() => {
                self.fail("The room has closed".into())
            }
            "relay" => self.relay_note = format!("relay refused: {error}"),
            _ => {}
        }
    }
    fn answered(&mut self, path: &str, v: Value) {
        match path {
            "host" => {
                self.code = v["code"].as_str().unwrap_or_default().to_owned();
                self.key = v["key"].as_str().unwrap_or_default().to_owned();
                self.phase = Phase::Ready;
                // no address of its own from STUN: the one the Worker saw, with this socket's port (often right)
                if self.public.is_none() {
                    if let (Some(ip), Ok(local)) = (
                        v["you"].as_str().and_then(|s| s.parse::<std::net::Ipv4Addr>().ok()),
                        self.socket.local_addr(),
                    ) {
                        self.public = Some(SocketAddr::new(ip.into(), local.port()));
                        self.post("addrs", json!({"code": self.code, "key": self.key, "addrs": self.own_addrs()}));
                    }
                }
            }
            "join" => {
                self.key = v["key"].as_str().unwrap_or_default().to_owned();
                self.candidates = parse_addrs(&v["host"]["addrs"]).0;
                self.joined = Some(Instant::now());
                self.phase = Phase::Ready;
            }
            "poll" if self.hosting => {
                let mut present = vec![];
                for m in v["members"].as_array().into_iter().flatten() {
                    let Some(slot) = m["slot"].as_u64().map(|s| s as u32) else {
                        continue;
                    };
                    present.push(slot);
                    if self.guests.contains_key(&slot) {
                        continue;
                    }
                    let (addrs, seen) = parse_addrs(&m["addrs"]);
                    let addrs: Vec<SocketAddr> = addrs.into_iter().map(|(a, _)| a).collect();
                    let mut ips: Vec<IpAddr> = addrs.iter().map(SocketAddr::ip).chain(seen).collect();
                    ips.sort();
                    ips.dedup();
                    if let Some(turn) = &mut self.turn {
                        for ip in &ips {
                            turn.permit(*ip);
                        }
                    }
                    self.guests.insert(
                        slot,
                        Guest {
                            name: m["name"].as_str().unwrap_or("Skater").to_owned(),
                            addrs,
                            ips,
                            appeared: Instant::now(),
                            path: None,
                        },
                    );
                }
                self.guests.retain(|slot, _| present.contains(slot));
            }
            "poll" => {
                for c in parse_addrs(&v["host"]["addrs"]).0 {
                    if !self.candidates.contains(&c) {
                        self.candidates.push(c);
                    }
                }
            }
            "relay" => {
                if let Some(server) = v["turn"]["server"].as_str() {
                    self.relay_note = "relay: connecting".into();
                    self.client.lock().unwrap().resolve("turn", vec![server.to_owned()]);
                    self.pending_login = Some((
                        v["turn"]["username"].as_str().unwrap_or_default().to_owned(),
                        v["turn"]["credential"].as_str().unwrap_or_default().to_owned(),
                    ));
                } else {
                    self.relay_note = format!("no relay: {}", v["noRelay"].as_str().unwrap_or("not offered"));
                }
            }
            _ => {}
        }
    }
    fn start_relay(&mut self, server: Option<SocketAddr>) {
        let (Some(server), Some((username, credential))) = (server, self.pending_login.take()) else {
            self.relay_note = "no relay: its server's name did not resolve".into();
            return;
        };
        let mut turn = Turn::new(server, &username, &credential, super::unique());
        for g in self.guests.values() {
            for ip in &g.ips {
                turn.permit(*ip);
            }
        }
        self.turn = Some(turn);
    }
    fn connected(&self) -> bool {
        if self.hosting {
            self.guests.values().any(|g| g.path.is_some())
        } else {
            self.host_path.is_some()
        }
    }
    /// The guest a datagram from `from` belongs to: by its exact address, else by an IP only it has.
    fn guest_of(&self, from: SocketAddr) -> Option<u32> {
        if let Some((&slot, _)) = self
            .guests
            .iter()
            .find(|(_, g)| g.addrs.contains(&from) || g.path.is_some_and(|p| p.addr == from))
        {
            return Some(slot);
        }
        let mut by_ip = self.guests.iter().filter(|(_, g)| g.ips.contains(&from.ip()));
        match (by_ip.next(), by_ip.next()) {
            (Some((&slot, _)), None) => Some(slot),
            _ => None,
        }
    }
    /// A datagram from a peer (directly, or out of the relay): the game's, or a punch.
    fn arrived(&mut self, from: SocketAddr, relay: bool, data: Vec<u8>, out: &mut Vec<(u64, Vec<u8>)>) {
        let punch = data.starts_with(PUNCH);
        let now = Instant::now();
        let peer = if self.hosting {
            let Some(slot) = self.guest_of(from) else {
                return;
            };
            let g = self.guests.get_mut(&slot).unwrap();
            if Path::better(g.path, relay) {
                g.path = Some(Path { addr: from, relay, heard: now });
            } else if let Some(p) = g.path.as_mut().filter(|p| p.addr == from) {
                p.heard = now;
            }
            GUEST + slot as u64
        } else {
            // a guest hears only the host: by one of its addresses (the relay's included)
            let Some(&(_, via_relay)) = self.candidates.iter().find(|(a, _)| *a == from) else {
                return;
            };
            if Path::better(self.host_path, via_relay) {
                self.host_path = Some(Path { addr: from, relay: via_relay, heard: now });
            } else if let Some(p) = self.host_path.as_mut().filter(|p| p.addr == from) {
                p.heard = now;
            }
            HOST
        };
        if !punch {
            out.push((peer, data));
        }
    }
    fn tick(&mut self) {
        let now = Instant::now();
        // STUN: each server in turn, three tries each; then on without a public address if none answered
        if self.phase == Phase::Stun {
            if self.public.is_some() {
                self.register();
            } else if now - self.stun_sent >= STUN_RETRY {
                let index = (self.stun_tries / 3) as usize;
                if let Some(server) = self.stun_servers.get(index) {
                    let _ = self.socket.send_to(&stun::binding_request(self.stun_transaction), server);
                    self.stun_tries += 1;
                    self.stun_sent = now;
                } else {
                    self.register();
                }
            }
        }
        if self.phase == Phase::Resolving && now - self.began > Duration::from_secs(8) {
            self.phase = Phase::Stun; // (the names did not resolve: nothing to ask, so straight on)
        }
        if self.phase != Phase::Ready {
            return;
        }
        let settled = if self.hosting {
            false // (the host's poll keeps the room alive)
        } else {
            self.host_path.is_some_and(|p| !p.relay)
        };
        let every = if settled { POLL_SETTLED } else { POLL_EVERY };
        if !self.poll_waiting && now - self.last_poll >= every {
            self.post("poll", json!({"code": self.code, "key": self.key}));
            self.poll_waiting = true;
            self.last_poll = now;
        }
        // punches: towards every address of whoever has not been heard from directly
        if now - self.last_punch >= PUNCH_EVERY {
            self.last_punch = now;
            let mut targets = vec![];
            if self.hosting {
                for g in self.guests.values() {
                    let direct = g.path.is_some_and(|p| !p.relay);
                    let age = now - g.appeared;
                    if !direct && age < GIVE_UP * 4 && (age < Duration::from_secs(10) || self.slow_tick(now)) {
                        targets.extend(g.addrs.iter().copied());
                    }
                }
            } else if let Some(joined) = self.joined {
                let direct = self.host_path.is_some_and(|p| !p.relay);
                let age = now - joined;
                if !direct && (age < Duration::from_secs(10) || self.slow_tick(now)) {
                    targets.extend(self.candidates.iter().filter(|(_, r)| !r).map(|(a, _)| *a));
                }
                if self.host_path.is_none() && age > GIVE_UP {
                    self.status = format!("Could not reach the host of room {}", self.code);
                }
            }
            for t in targets {
                let _ = self.socket.send_to(PUNCH, t);
            }
        }
        // the relay: asked for once a guest has stayed out of reach
        if self.hosting
            && !self.relay_asked
            && self.guests.values().any(|g| g.path.is_none() && now - g.appeared >= RELAY_AFTER)
        {
            self.relay_asked = true;
            self.relay_note = "relay: asking".into();
            self.post("relay", json!({"code": self.code, "key": self.key}));
        }
        if let Some(turn) = &mut self.turn {
            for d in turn.poll(now) {
                let _ = self.socket.send_to(&d, turn.server());
            }
            if let Some(why) = turn.failed() {
                self.relay_note = format!("no relay: {why}");
            } else if turn.relayed().is_some() && !self.relay_published {
                self.relay_published = true;
                self.relay_note = "relay ready".into();
                self.post("addrs", json!({"code": self.code, "key": self.key, "addrs": self.own_addrs()}));
            }
        }
        self.status = self.describe();
    }
    /// Once a second, for punches that keep trying slowly.
    fn slow_tick(&self, now: Instant) -> bool {
        (now - self.began).as_millis() % PUNCH_SLOW.as_millis() < PUNCH_EVERY.as_millis()
    }
    fn describe(&self) -> String {
        if self.hosting {
            let direct = self.guests.values().filter(|g| g.path.is_some_and(|p| !p.relay)).count();
            let relay = self.guests.values().filter(|g| g.path.is_some_and(|p| p.relay)).count();
            let waiting: Vec<&str> = self
                .guests
                .values()
                .filter(|g| g.path.is_none())
                .map(|g| g.name.as_str())
                .collect();
            let mut s = format!("Room {} | {} of {} skaters", self.code, 1 + direct + relay, self.max);
            if relay > 0 {
                s += &format!(" | {relay} by relay");
            }
            if !waiting.is_empty() {
                s += &format!(" | connecting: {}", waiting.join(", "));
            }
            s
        } else {
            match self.host_path {
                Some(p) if p.relay => format!("Room {} | connected by relay", self.code),
                Some(_) => format!("Room {} | connected directly", self.code),
                None if self.joined.is_some_and(|j| j.elapsed() > GIVE_UP) => {
                    format!("Could not reach the host of room {}", self.code)
                }
                None => format!("Room {} | reaching the host...", self.code),
            }
        }
    }
}

impl Transport for Rooms {
    fn send(&mut self, peer: u64, data: &[u8]) -> io::Result<()> {
        let blocked = || io::Error::new(io::ErrorKind::WouldBlock, "not connected yet");
        if self.hosting {
            let slot = peer.checked_sub(GUEST).ok_or_else(blocked)? as u32;
            let path = self.guests.get(&slot).and_then(|g| g.path).ok_or_else(blocked)?;
            if path.relay {
                let turn = self.turn.as_mut().ok_or_else(blocked)?;
                let wrapped = turn.send(path.addr, data).ok_or_else(blocked)?;
                return self.socket.send_to(&wrapped, turn.server()).map(|_| ());
            }
            return self.socket.send_to(data, path.addr).map(|_| ());
        }
        if peer != HOST {
            return Err(blocked());
        }
        if let Some(p) = self.host_path.filter(|p| p.heard.elapsed() < PATH_STALE) {
            return self.socket.send_to(data, p.addr).map(|_| ());
        }
        // not (or no longer) heard from: every address at once, the relay's once a direct path had its chance
        let relay_ok = self.joined.is_some_and(|j| j.elapsed() >= RELAY_DELAY);
        let targets: Vec<SocketAddr> = self
            .candidates
            .iter()
            .filter(|(_, relay)| !relay || relay_ok)
            .map(|(a, _)| *a)
            .collect();
        if targets.is_empty() {
            return Err(blocked());
        }
        for t in targets {
            self.socket.send_to(data, t)?;
        }
        Ok(())
    }
    fn receive(&mut self) -> io::Result<Vec<(u64, Vec<u8>)>> {
        self.answers();
        let mut out = vec![];
        let mut buffer = [0; 1600];
        for _ in 0..4096 {
            let (n, from) = match self.socket.recv_from(&mut buffer) {
                Ok(r) => r,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    break;
                }
                Err(e) => return Err(e),
            };
            let data = &buffer[..n];
            if self.phase == Phase::Stun && stun::is_stun(data) {
                if let Some(mapped) = stun::mapped(data, &self.stun_transaction) {
                    self.public = Some(mapped);
                }
                continue;
            }
            if let Some(turn) = self.turn.as_mut().filter(|t| t.server() == from) {
                if let Some((peer, inner)) = turn.receive(data, Instant::now()) {
                    if inner.len() <= skate_net::packed::MTU {
                        self.arrived(peer, true, inner, &mut out);
                    }
                }
                continue;
            }
            if n <= skate_net::packed::MTU {
                self.arrived(from, false, data.to_vec(), &mut out);
            }
        }
        self.tick();
        Ok(out)
    }
    fn status(&self) -> String {
        self.status.clone()
    }
    fn metrics(&self) -> String {
        self.relay_note.clone()
    }
    fn room_code(&self) -> Option<String> {
        (self.phase == Phase::Ready && !self.code.is_empty()).then(|| self.code.clone())
    }
    fn failed(&self) -> bool {
        self.phase == Phase::Failed
    }
}

impl Drop for Rooms {
    fn drop(&mut self) {
        if let Some(close) = self.turn.as_mut().and_then(Turn::close) {
            let server = self.turn.as_ref().unwrap().server();
            let _ = self.socket.send_to(&close, server);
        }
        if !self.key.is_empty() {
            // (the thread finishes this after the Client is dropped)
            self.post("leave", json!({"code": self.code, "key": self.key}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skate_net::lobby::{Info, Session};

    fn info(id: u64) -> Info {
        Info { id, map: 1, rig: 2, physics: 3, appearance: 4 }
    }
    /// Pumps transports and sessions the way the game's `receive` system does, with the host's code naming the
    /// session once it is known.
    fn pump(t: &mut Rooms, s: &mut Session, now: u64) {
        if let Some(code) = t.room_code().filter(|_| s.is_host()) {
            s.session = rooms::session(&code);
        }
        for (peer, p) in t.receive().unwrap() {
            s.receive(peer, &p, now);
        }
        for p in s.service(now) {
            let _ = t.send(p.peer, &p.data);
        }
    }

    /// A host and two guests meet through a matchmaking Worker: real STUN, HTTP and UDP on this machine. Needs
    /// `node matchmaking/dev-server.mjs` running and SK8_MATCH_URL=http://127.0.0.1:8787.
    #[test]
    #[ignore]
    fn three_skaters_meet_through_the_worker() {
        let url = std::env::var("SK8_MATCH_URL").expect("SK8_MATCH_URL");
        let mut host = Rooms::new(url.clone(), Start::Host { max: 3 }, "Host").unwrap();
        let mut hs = Session::new(0, info(11), None);
        hs.publish_application("@name", b"Host".to_vec(), 0);
        let began = Instant::now();
        let code = loop {
            pump(&mut host, &mut hs, began.elapsed().as_millis() as u64);
            if let Some(code) = host.room_code() {
                break code;
            }
            assert!(!host.failed(), "{}", host.status());
            assert!(began.elapsed() < Duration::from_secs(15), "no room: {}", host.status());
            std::thread::sleep(Duration::from_millis(10));
        };
        eprintln!("room {code}, host public address {:?}, lan {:?}", host.public, host.lan);
        let mut guests: Vec<(Rooms, Session)> = (0..2)
            .map(|i| {
                let t = Rooms::new(url.clone(), Start::Join(code.to_lowercase()), &format!("Guest{i}")).unwrap();
                let mut s = Session::new(rooms::session(&code), info(21 + i), Some(HOST));
                s.publish_application("@name", format!("Guest{i}").into_bytes(), 0);
                (t, s)
            })
            .collect();
        loop {
            let now = began.elapsed().as_millis() as u64;
            pump(&mut host, &mut hs, now);
            for (t, s) in &mut guests {
                pump(t, s, now);
                assert!(!t.failed(), "{}", t.status());
            }
            // everyone knows all three skaters and their names: the host forwarded the roster and the records
            let named = |s: &Session| s.actors.values().filter(|a| a.application.contains_key("@name")).count() == 3;
            if hs.actors.len() == 3
                && named(&hs)
                && guests.iter().all(|(_, s)| s.connected() && s.actors.len() == 3 && named(s))
            {
                break;
            }
            assert!(began.elapsed() < Duration::from_secs(30), "host: {} | guests: {} / {}",
                host.status(), guests[0].0.status(), guests[1].0.status());
            std::thread::sleep(Duration::from_millis(10));
        }
        eprintln!("{} | {} | {}", host.status(), guests[0].0.status(), guests[1].0.status());
        assert!(host.status().contains("3 of 3"));
        // a guest knows the other guest's name, which only the host could have passed on
        assert_eq!(guests[0].1.actors[&22].application["@name"].value.as_slice(), b"Guest1");
        assert!(guests.iter().all(|(t, _)| t.status().contains("connected directly")));
        let full = Rooms::new(url, Start::Join(code), "Fourth").unwrap();
        let mut full = full;
        let mut fs = Session::new(0, info(99), Some(HOST));
        while !full.failed() {
            pump(&mut full, &mut fs, began.elapsed().as_millis() as u64);
            assert!(began.elapsed() < Duration::from_secs(45));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(full.status(), "That room is full");
    }
}
