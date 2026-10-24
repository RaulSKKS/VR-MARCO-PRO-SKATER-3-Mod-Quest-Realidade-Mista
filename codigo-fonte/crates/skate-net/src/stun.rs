//! STUN messages (RFC 5389): the binding request that tells a socket the address its NAT shows the internet, and
//! the attribute layer TURN (RFC 5766, `turn.rs`) is built on. No I/O here.
use hmac::{Hmac, Mac};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub const COOKIE: u32 = 0x2112_A442;
pub const BINDING: u16 = 0x0001;
// classes, ORed into a method
pub const INDICATION: u16 = 0x0010;
pub const SUCCESS: u16 = 0x0100;
pub const ERROR: u16 = 0x0110;
// attributes
pub const MAPPED_ADDRESS: u16 = 0x0001;
pub const USERNAME: u16 = 0x0006;
pub const MESSAGE_INTEGRITY: u16 = 0x0008;
pub const ERROR_CODE: u16 = 0x0009;
pub const CHANNEL_NUMBER: u16 = 0x000C;
pub const LIFETIME: u16 = 0x000D;
pub const XOR_PEER_ADDRESS: u16 = 0x0012;
pub const DATA: u16 = 0x0013;
pub const REALM: u16 = 0x0014;
pub const NONCE: u16 = 0x0015;
pub const XOR_RELAYED_ADDRESS: u16 = 0x0016;
pub const REQUESTED_TRANSPORT: u16 = 0x0019;
pub const XOR_MAPPED_ADDRESS: u16 = 0x0020;
pub const SOFTWARE: u16 = 0x8022;
pub const FINGERPRINT: u16 = 0x8028;

pub struct Message {
    pub kind: u16,
    pub transaction: [u8; 12],
    pub attrs: Vec<(u16, Vec<u8>)>,
}

/// Whether a datagram is a STUN message (and not game data or TURN ChannelData).
pub fn is_stun(data: &[u8]) -> bool {
    data.len() >= 20 && data[0] & 0xC0 == 0 && data[4..8] == COOKIE.to_be_bytes()
}

/// The long-term credential key (RFC 5389 15.4): MD5 of "username:realm:password".
pub fn long_term_key(username: &str, realm: &str, password: &str) -> Vec<u8> {
    use md5::Digest;
    md5::Md5::digest(format!("{username}:{realm}:{password}").as_bytes()).to_vec()
}

fn xor_address(addr: SocketAddr, transaction: &[u8; 12]) -> Vec<u8> {
    let port = addr.port() ^ (COOKIE >> 16) as u16;
    let mut v = vec![0];
    match addr.ip() {
        IpAddr::V4(ip) => {
            v.push(1);
            v.extend(port.to_be_bytes());
            v.extend((u32::from(ip) ^ COOKIE).to_be_bytes());
        }
        IpAddr::V6(ip) => {
            v.push(2);
            v.extend(port.to_be_bytes());
            let mut mask = COOKIE.to_be_bytes().to_vec();
            mask.extend(transaction);
            v.extend(ip.octets().iter().zip(mask).map(|(a, b)| a ^ b));
        }
    }
    v
}

fn read_address(v: &[u8], transaction: &[u8; 12], xor: bool) -> Option<SocketAddr> {
    if v.len() < 8 {
        return None;
    }
    let mut port = u16::from_be_bytes([v[2], v[3]]);
    let mut mask = COOKIE.to_be_bytes().to_vec();
    mask.extend(transaction);
    if !xor {
        mask.iter_mut().for_each(|b| *b = 0);
    } else {
        port ^= (COOKIE >> 16) as u16;
    }
    match v[1] {
        1 => {
            let o: [u8; 4] = std::array::from_fn(|i| v[4 + i] ^ mask[i]);
            Some(SocketAddr::new(Ipv4Addr::from(o).into(), port))
        }
        2 if v.len() >= 20 => {
            let o: [u8; 16] = std::array::from_fn(|i| v[4 + i] ^ mask[i]);
            Some(SocketAddr::new(Ipv6Addr::from(o).into(), port))
        }
        _ => None,
    }
}

fn integrity_of(before: &[u8], key: &[u8]) -> [u8; 20] {
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(key).expect("any key length");
    mac.update(before);
    mac.finalize().into_bytes().into()
}

fn fingerprint_of(before: &[u8]) -> u32 {
    crc32fast::hash(before) ^ 0x5354_554E
}

impl Message {
    pub fn new(kind: u16, transaction: [u8; 12]) -> Self {
        Self {
            kind,
            transaction,
            attrs: vec![],
        }
    }
    pub fn add(&mut self, kind: u16, value: impl Into<Vec<u8>>) -> &mut Self {
        self.attrs.push((kind, value.into()));
        self
    }
    pub fn add_address(&mut self, kind: u16, addr: SocketAddr) -> &mut Self {
        let v = xor_address(addr, &self.transaction);
        self.add(kind, v)
    }
    /// The message as sent; with `integrity` (the key) a MESSAGE-INTEGRITY attribute, and a FINGERPRINT if asked.
    pub fn encode(&self, integrity: Option<&[u8]>, fingerprint: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(128);
        out.extend(self.kind.to_be_bytes());
        out.extend([0, 0]);
        out.extend(COOKIE.to_be_bytes());
        out.extend(self.transaction);
        for (kind, value) in &self.attrs {
            out.extend(kind.to_be_bytes());
            out.extend((value.len() as u16).to_be_bytes());
            out.extend(value);
            out.resize(out.len().next_multiple_of(4), 0);
        }
        let set_length = |out: &mut Vec<u8>, extra: usize| {
            let n = (out.len() - 20 + extra) as u16;
            out[2..4].copy_from_slice(&n.to_be_bytes());
        };
        if let Some(key) = integrity {
            // the length counts the integrity attribute while the HMAC covers everything before it
            set_length(&mut out, 24);
            let mac = integrity_of(&out, key);
            out.extend(MESSAGE_INTEGRITY.to_be_bytes());
            out.extend(20u16.to_be_bytes());
            out.extend(mac);
        }
        if fingerprint {
            set_length(&mut out, 8);
            let crc = fingerprint_of(&out);
            out.extend(FINGERPRINT.to_be_bytes());
            out.extend(4u16.to_be_bytes());
            out.extend(crc.to_be_bytes());
        }
        set_length(&mut out, 0);
        out
    }
    pub fn decode(data: &[u8]) -> Option<Self> {
        if !is_stun(data) {
            return None;
        }
        let length = u16::from_be_bytes([data[2], data[3]]) as usize;
        if data.len() < 20 + length || length % 4 != 0 {
            return None;
        }
        let mut message = Self::new(
            u16::from_be_bytes([data[0], data[1]]),
            data[8..20].try_into().unwrap(),
        );
        let mut at = 20;
        while at + 4 <= 20 + length {
            let kind = u16::from_be_bytes([data[at], data[at + 1]]);
            let n = u16::from_be_bytes([data[at + 2], data[at + 3]]) as usize;
            let value = data.get(at + 4..at + 4 + n)?;
            message.attrs.push((kind, value.to_vec()));
            at += 4 + n.next_multiple_of(4);
        }
        Some(message)
    }
    pub fn get(&self, kind: u16) -> Option<&[u8]> {
        self.attrs
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, v)| v.as_slice())
    }
    /// An address attribute: XOR-encoded ones decoded, MAPPED-ADDRESS (old servers) as it is.
    pub fn address(&self, kind: u16) -> Option<SocketAddr> {
        read_address(self.get(kind)?, &self.transaction, kind != MAPPED_ADDRESS)
    }
    /// The error code of an error response (401, 438, ...).
    pub fn error(&self) -> Option<u16> {
        let v = self.get(ERROR_CODE)?;
        (v.len() >= 4).then(|| (v[2] & 7) as u16 * 100 + v[3] as u16)
    }
    pub fn method(&self) -> u16 {
        self.kind & !0x0110
    }
    pub fn class(&self) -> u16 {
        self.kind & 0x0110
    }
}

/// A binding request; the answer's XOR-MAPPED-ADDRESS (`mapped`) is this socket as the internet sees it.
pub fn binding_request(transaction: [u8; 12]) -> Vec<u8> {
    Message::new(BINDING, transaction).encode(None, true)
}

/// The address a binding response for `transaction` reports, if `data` is one.
pub fn mapped(data: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr> {
    let m = Message::decode(data)?;
    if m.kind != BINDING | SUCCESS || &m.transaction != transaction {
        return None;
    }
    m.address(XOR_MAPPED_ADDRESS)
        .or_else(|| m.address(MAPPED_ADDRESS))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // RFC 5769 2.2: sample IPv4 response, short-term password "VOkJxbRl1RmTxUk/WvJxBt"
    const RESPONSE: &str = "0101003c 2112a442 b7e7a701 bc34d686 fa87dfae 8022000b 74657374 20766563 746f7220
        00200008 0001a147 e112a643 00080014 2b91f599 fd9e90c3 8c7489f9 2af9ba53 f06be7d7 80280004 c07d4c96";

    #[test]
    fn rfc5769_response_decodes() {
        let data = hex(RESPONSE);
        let m = Message::decode(&data).unwrap();
        assert_eq!(m.kind, BINDING | SUCCESS);
        assert_eq!(m.get(SOFTWARE), Some(b"test vector".as_slice()));
        assert_eq!(
            m.address(XOR_MAPPED_ADDRESS),
            Some("192.0.2.1:32853".parse().unwrap())
        );
        assert_eq!(
            mapped(&data, &m.transaction),
            Some("192.0.2.1:32853".parse().unwrap())
        );
    }

    #[test]
    fn rfc5769_integrity_and_fingerprint() {
        // (the vector pads SOFTWARE with a space where this encoder pads with zero, so the two checks run on its
        // own bytes: the HMAC over everything before MESSAGE-INTEGRITY with the length counting it, the CRC over
        // everything before FINGERPRINT)
        let data = hex(RESPONSE);
        let mut before = data[..48].to_vec();
        before[2..4].copy_from_slice(&(48u16 - 20 + 24).to_be_bytes());
        assert_eq!(integrity_of(&before, b"VOkJxbRl1RmTxUk/WvJxBt").as_slice(), &data[52..72]);
        assert_eq!(fingerprint_of(&data[..72]).to_be_bytes(), data[76..80]);
        // and what this encoder makes of the same message carries the same attributes, verifiably
        let mut m = Message::new(BINDING | SUCCESS, data[8..20].try_into().unwrap());
        m.add(SOFTWARE, b"test vector".to_vec());
        m.add_address(XOR_MAPPED_ADDRESS, "192.0.2.1:32853".parse().unwrap());
        let mine = m.encode(Some(b"VOkJxbRl1RmTxUk/WvJxBt"), true);
        assert_eq!(mine.len(), data.len());
        let mut before = mine[..48].to_vec();
        before[2..4].copy_from_slice(&(48u16 - 20 + 24).to_be_bytes());
        assert_eq!(integrity_of(&before, b"VOkJxbRl1RmTxUk/WvJxBt").as_slice(), &mine[52..72]);
        assert_eq!(fingerprint_of(&mine[..72]).to_be_bytes(), mine[76..80]);
    }

    #[test]
    fn ipv6_address_round_trips() {
        let addr: SocketAddr = "[2001:db8:1234:5678:11:2233:4455:6677]:32853".parse().unwrap();
        let mut m = Message::new(BINDING | SUCCESS, [7; 12]);
        m.add_address(XOR_MAPPED_ADDRESS, addr);
        let back = Message::decode(&m.encode(None, false)).unwrap();
        assert_eq!(back.address(XOR_MAPPED_ADDRESS), Some(addr));
    }

    #[test]
    fn error_code_and_classes() {
        let mut m = Message::new(0x0003 | ERROR, [1; 12]);
        m.add(ERROR_CODE, vec![0, 0, 4, 1]);
        let m = Message::decode(&m.encode(None, false)).unwrap();
        assert_eq!(m.error(), Some(401));
        assert_eq!(m.method(), 0x0003);
        assert_eq!(m.class(), ERROR);
        assert!(!is_stun(b"SK8NET01 game data here"));
    }
}
