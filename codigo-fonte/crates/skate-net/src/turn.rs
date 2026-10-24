//! A minimal TURN client (RFC 5766) over UDP, for the host of an online room: it gets a relayed address from a
//! TURN server (Cloudflare's, with a short-lived login from the matchmaking Worker), lets the room's guests through
//! by their IP addresses, and wraps and unwraps game datagrams. No I/O: the caller sends what `poll`/`send` return
//! to `server()` from the game's own socket and hands back every datagram that came from there.
//!
//! The sequence follows Tenkaichi3Decomp's relay (port/src/gs/net_relay.c): an Allocate is challenged with 401 and
//! sent again with the long-term credential; permissions are renewed every minute, the allocation every two;
//! channels (0x4000 up) replace Send/Data indications once bound; an Allocate nobody answers is tried again on the
//! other of ports 3478 and 53; a Refresh with lifetime 0 ends it all.
use crate::stun::{self, Message};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

const ALLOCATE: u16 = 0x0003;
const REFRESH: u16 = 0x0004;
const SEND: u16 = 0x0006;
const DATA: u16 = 0x0007;
const CREATE_PERMISSION: u16 = 0x0008;
const CHANNEL_BIND: u16 = 0x0009;
const RETRY: Duration = Duration::from_millis(500);
const TRIES: u32 = 7;
const REFRESH_EVERY: Duration = Duration::from_secs(120);
const PERMISSION_EVERY: Duration = Duration::from_secs(60);
const CHANNEL_EVERY: Duration = Duration::from_secs(300);
const SOFTWARE: &[u8] = b"skate3-port";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Purpose {
    Allocate,
    Refresh,
    Permission,
    Channel(u16, SocketAddr),
}
struct Pending {
    purpose: Purpose,
    sent: Instant,
    tries: u32,
    data: Vec<u8>,
}
pub struct Turn {
    server: SocketAddr,
    username: String,
    credential: String,
    realm: String,
    nonce: Vec<u8>,
    key: Option<Vec<u8>>,
    relayed: Option<SocketAddr>,
    failed: Option<String>,
    pending: BTreeMap<[u8; 12], Pending>,
    permissions: BTreeMap<IpAddr, Option<Instant>>,
    channels: BTreeMap<SocketAddr, (u16, Option<Instant>)>,
    peers: BTreeMap<u16, SocketAddr>,
    next_channel: u16,
    last_refresh: Instant,
    last_permission: Instant,
    unanswered: u32,
    counter: u64,
    seed: u64,
}

impl Turn {
    /// `server` is "ip:port" (resolved by the caller), `username`/`credential` the login.
    pub fn new(server: SocketAddr, username: &str, credential: &str, seed: u64) -> Self {
        let mut turn = Self {
            server,
            username: username.into(),
            credential: credential.into(),
            realm: String::new(),
            nonce: vec![],
            key: None,
            relayed: None,
            failed: None,
            pending: BTreeMap::new(),
            permissions: BTreeMap::new(),
            channels: BTreeMap::new(),
            peers: BTreeMap::new(),
            next_channel: 0x4000,
            last_refresh: Instant::now(),
            last_permission: Instant::now(),
            unanswered: 0,
            counter: 0,
            seed,
        };
        turn.allocate();
        turn
    }
    /// Where every datagram for the TURN server goes (it can change: see `poll`).
    pub fn server(&self) -> SocketAddr {
        self.server
    }
    /// The relayed address guests reach the host at, once allocated.
    pub fn relayed(&self) -> Option<SocketAddr> {
        self.relayed
    }
    pub fn failed(&self) -> Option<&str> {
        self.failed.as_deref()
    }
    fn transaction(&mut self) -> [u8; 12] {
        self.counter += 1;
        let h = crate::hash(&[self.seed.to_le_bytes(), self.counter.to_le_bytes()].concat());
        let mut t = [0; 12];
        t[..8].copy_from_slice(&h.to_le_bytes());
        t[8..].copy_from_slice(&(self.counter as u32).to_le_bytes());
        t
    }
    fn request(&mut self, method: u16, purpose: Purpose, build: impl FnOnce(&mut Message)) {
        let transaction = self.transaction();
        let mut m = Message::new(method, transaction);
        build(&mut m);
        if self.key.is_some() {
            m.add(stun::USERNAME, self.username.as_bytes().to_vec());
            m.add(stun::REALM, self.realm.as_bytes().to_vec());
            m.add(stun::NONCE, self.nonce.clone());
        }
        let data = m.encode(self.key.as_deref(), true);
        self.pending.insert(
            transaction,
            Pending {
                purpose,
                sent: Instant::now() - RETRY, // (sent by the next poll)
                tries: 0,
                data,
            },
        );
    }
    fn allocate(&mut self) {
        self.request(ALLOCATE, Purpose::Allocate, |m| {
            m.add(stun::REQUESTED_TRANSPORT, vec![17, 0, 0, 0]); // UDP
            m.add(stun::SOFTWARE, SOFTWARE.to_vec());
        });
    }
    fn create_permissions(&mut self) {
        let ips: Vec<IpAddr> = self.permissions.keys().copied().collect();
        if ips.is_empty() {
            return;
        }
        self.request(CREATE_PERMISSION, Purpose::Permission, |m| {
            for ip in ips {
                m.add_address(stun::XOR_PEER_ADDRESS, SocketAddr::new(ip, 0));
            }
        });
        self.last_permission = Instant::now();
    }
    fn bind(&mut self, peer: SocketAddr, channel: u16) {
        self.request(CHANNEL_BIND, Purpose::Channel(channel, peer), |m| {
            m.add(stun::CHANNEL_NUMBER, [(channel >> 8) as u8, channel as u8, 0, 0].to_vec());
            m.add_address(stun::XOR_PEER_ADDRESS, peer);
        });
    }
    /// Lets datagrams from this IP address through the relay (a guest's address as the Worker saw it).
    pub fn permit(&mut self, ip: IpAddr) {
        if self.permissions.contains_key(&ip) {
            return;
        }
        self.permissions.insert(ip, None);
        if self.relayed.is_some() {
            self.create_permissions();
        }
    }
    /// What is due to the server now: first sends, retransmissions and renewals.
    pub fn poll(&mut self, now: Instant) -> Vec<Vec<u8>> {
        if self.failed.is_some() {
            return vec![];
        }
        if self.relayed.is_some() {
            if now - self.last_refresh >= REFRESH_EVERY {
                self.last_refresh = now;
                self.request(REFRESH, Purpose::Refresh, |m| {
                    m.add(stun::LIFETIME, 600u32.to_be_bytes().to_vec());
                });
            }
            if now - self.last_permission >= PERMISSION_EVERY {
                self.create_permissions();
            }
            let stale: Vec<(SocketAddr, u16)> = self
                .channels
                .iter()
                .filter(|(_, (_, bound))| bound.is_some_and(|t| now - t >= CHANNEL_EVERY))
                .map(|(&peer, &(channel, _))| (peer, channel))
                .collect();
            for (peer, channel) in stale {
                self.channels.insert(peer, (channel, Some(now)));
                self.bind(peer, channel);
            }
        }
        let mut out = vec![];
        let mut gave_up = None;
        for p in self.pending.values_mut() {
            if now - p.sent < RETRY {
                continue;
            }
            if p.tries >= TRIES {
                gave_up = Some(p.purpose);
                continue;
            }
            p.tries += 1;
            p.sent = now;
            out.push(p.data.clone());
        }
        if let Some(purpose) = gave_up {
            self.pending.retain(|_, p| p.tries < TRIES);
            if purpose == Purpose::Allocate {
                self.failed = Some("the relay server did not answer".into());
            }
        }
        // An Allocate nobody answers: the other port (some networks let only one of 3478 and 53 out).
        if self.relayed.is_none() && self.key.is_none() {
            if let Some(p) = self.pending.values().find(|p| p.purpose == Purpose::Allocate) {
                if p.tries >= 3 && self.unanswered == 0 {
                    self.unanswered = 1;
                    self.server.set_port(if self.server.port() == 53 { 3478 } else { 53 });
                }
            }
        }
        out
    }
    /// A datagram that came from `server()`: a response (handled here) or relayed game data, returned with the
    /// address of the guest that sent it.
    pub fn receive(&mut self, data: &[u8], now: Instant) -> Option<(SocketAddr, Vec<u8>)> {
        if data.len() >= 4 && (0x40..0x80).contains(&data[0]) {
            let channel = u16::from_be_bytes([data[0], data[1]]);
            let n = u16::from_be_bytes([data[2], data[3]]) as usize;
            let peer = *self.peers.get(&channel)?;
            return Some((peer, data.get(4..4 + n)?.to_vec()));
        }
        let m = Message::decode(data)?;
        if m.kind == DATA | stun::INDICATION {
            return Some((m.address(stun::XOR_PEER_ADDRESS)?, m.get(stun::DATA)?.to_vec()));
        }
        let pending = self.pending.remove(&m.transaction)?;
        match m.class() {
            stun::SUCCESS => match pending.purpose {
                Purpose::Allocate => {
                    self.relayed = m.address(stun::XOR_RELAYED_ADDRESS);
                    if self.relayed.is_none() {
                        self.failed = Some("the relay server gave no address".into());
                    }
                    self.last_refresh = now;
                    self.create_permissions();
                }
                Purpose::Permission => {
                    for t in self.permissions.values_mut() {
                        *t = Some(now);
                    }
                }
                Purpose::Channel(channel, peer) => {
                    self.peers.insert(channel, peer);
                    self.channels.insert(peer, (channel, Some(now)));
                }
                Purpose::Refresh => {}
            },
            stun::ERROR => {
                let code = m.error().unwrap_or(0);
                let challenged = code == 401 && self.key.is_none();
                if challenged || code == 438 {
                    // the challenge (or a stale nonce): the same request again with the credential
                    if let Some(realm) = m.get(stun::REALM) {
                        self.realm = String::from_utf8_lossy(realm).into();
                    }
                    if let Some(nonce) = m.get(stun::NONCE) {
                        self.nonce = nonce.to_vec();
                    }
                    self.key = Some(stun::long_term_key(&self.username, &self.realm, &self.credential));
                    match pending.purpose {
                        Purpose::Allocate => self.allocate(),
                        Purpose::Refresh => self.request(REFRESH, Purpose::Refresh, |m| {
                            m.add(stun::LIFETIME, 600u32.to_be_bytes().to_vec());
                        }),
                        Purpose::Permission => self.create_permissions(),
                        Purpose::Channel(channel, peer) => self.bind(peer, channel),
                    }
                } else if pending.purpose == Purpose::Allocate {
                    self.failed = Some(format!("the relay server refused the login ({code})"));
                }
            }
            _ => {}
        }
        None
    }
    /// A game datagram for a guest, wrapped for the server: ChannelData once the guest's channel is bound, a Send
    /// indication until then (which also asks for the channel). None before the allocation.
    pub fn send(&mut self, peer: SocketAddr, data: &[u8]) -> Option<Vec<u8>> {
        self.relayed?;
        match self.channels.get(&peer) {
            Some(&(channel, Some(_))) => {
                let mut out = Vec::with_capacity(4 + data.len());
                out.extend(channel.to_be_bytes());
                out.extend((data.len() as u16).to_be_bytes());
                out.extend(data);
                return Some(out);
            }
            None if self.next_channel < 0x7FFF => {
                let channel = self.next_channel;
                self.next_channel += 1;
                self.channels.insert(peer, (channel, None));
                self.bind(peer, channel);
            }
            _ => {}
        }
        let transaction = self.transaction();
        let mut m = Message::new(SEND | stun::INDICATION, transaction);
        m.add_address(stun::XOR_PEER_ADDRESS, peer);
        m.add(stun::DATA, data.to_vec());
        Some(m.encode(None, false))
    }
    /// The datagram that ends the allocation (a Refresh with lifetime 0); nothing if there is none.
    pub fn close(&mut self) -> Option<Vec<u8>> {
        self.relayed?;
        self.relayed = None;
        let transaction = self.transaction();
        let mut m = Message::new(REFRESH, transaction);
        m.add(stun::LIFETIME, 0u32.to_be_bytes().to_vec());
        m.add(stun::USERNAME, self.username.as_bytes().to_vec());
        m.add(stun::REALM, self.realm.as_bytes().to_vec());
        m.add(stun::NONCE, self.nonce.clone());
        Some(m.encode(self.key.as_deref(), true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A stand-in TURN server: challenges once, then grants everything.
    fn answer(request: &[u8], relayed: SocketAddr) -> Vec<u8> {
        let m = Message::decode(request).unwrap();
        let mut r;
        if m.get(stun::MESSAGE_INTEGRITY).is_none() && m.class() == 0 {
            r = Message::new(m.method() | stun::ERROR, m.transaction);
            r.add(stun::ERROR_CODE, vec![0, 0, 4, 1]);
            r.add(stun::REALM, b"example.org".to_vec());
            r.add(stun::NONCE, b"abc".to_vec());
        } else {
            r = Message::new(m.method() | stun::SUCCESS, m.transaction);
            if m.method() == ALLOCATE {
                r.add_address(stun::XOR_RELAYED_ADDRESS, relayed);
            }
        }
        r.encode(None, false)
    }

    #[test]
    fn allocates_after_challenge_and_relays() {
        let relayed: SocketAddr = "104.30.1.1:5000".parse().unwrap();
        let guest: SocketAddr = "203.0.113.9:8000".parse().unwrap();
        let mut t = Turn::new("141.101.90.1:3478".parse().unwrap(), "user", "pass", 1);
        let now = Instant::now() + Duration::from_secs(1); // (later than every request made in here)
        let first = t.poll(now);
        assert_eq!(first.len(), 1);
        assert!(t.receive(&answer(&first[0], relayed), now).is_none());
        let second = t.poll(now);
        let m = Message::decode(&second[0]).unwrap();
        assert!(m.get(stun::MESSAGE_INTEGRITY).is_some() && m.get(stun::USERNAME) == Some(b"user".as_slice()));
        t.receive(&answer(&second[0], relayed), now);
        assert_eq!(t.relayed(), Some(relayed));

        t.permit(guest.ip());
        let perm = t.poll(now);
        assert_eq!(Message::decode(&perm[0]).unwrap().method(), CREATE_PERMISSION);
        t.receive(&answer(&perm[0], relayed), now);

        // before the channel is bound: a Send indication, and a ChannelBind goes out
        let sent = t.send(guest, b"hello").unwrap();
        let m = Message::decode(&sent).unwrap();
        assert_eq!(m.kind, SEND | stun::INDICATION);
        assert_eq!(m.address(stun::XOR_PEER_ADDRESS), Some(guest));
        let bind = t.poll(now);
        assert_eq!(Message::decode(&bind[0]).unwrap().method(), CHANNEL_BIND);
        t.receive(&answer(&bind[0], relayed), now);
        // after: ChannelData both ways
        let sent = t.send(guest, b"hello").unwrap();
        assert_eq!(&sent[..4], &[0x40, 0x00, 0, 5]);
        assert_eq!(t.receive(&sent, now), Some((guest, b"hello".to_vec())));

        // a Data indication from the server is game data from the guest
        let mut d = Message::new(DATA | stun::INDICATION, [9; 12]);
        d.add_address(stun::XOR_PEER_ADDRESS, guest);
        d.add(stun::DATA, b"hi".to_vec());
        assert_eq!(t.receive(&d.encode(None, false), now), Some((guest, b"hi".to_vec())));

        assert!(t.close().is_some());
        assert!(t.close().is_none());
    }

    #[test]
    fn unanswered_allocate_tries_port_53_then_fails() {
        let mut t = Turn::new("141.101.90.1:3478".parse().unwrap(), "u", "p", 2);
        let mut now = Instant::now();
        for _ in 0..3 {
            t.poll(now);
            now += RETRY;
        }
        t.poll(now);
        assert_eq!(t.server().port(), 53);
        for _ in 0..TRIES {
            now += RETRY;
            t.poll(now);
        }
        assert!(t.failed().is_some());
    }
}
