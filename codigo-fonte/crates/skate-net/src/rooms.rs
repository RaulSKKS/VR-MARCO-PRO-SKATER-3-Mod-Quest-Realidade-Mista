//! The matchmaking Worker's client (matchmaking/worker.js): room codes for online free-skate. Requests run on a
//! thread of their own, one after another, so that the game's frame never waits on the network; answers come back
//! through `answers()`. Also resolves host names (STUN and TURN servers) there, for the same reason.
use serde_json::{Value, json};
use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::mpsc::{Receiver, Sender, channel},
    time::Duration,
};

/// The netcode version a room is made with: host and guests must agree (the Worker refuses others).
pub const VERSION: i64 = 1;
/// This build's release number, for the Worker's MIN_RELEASE.
pub const RELEASE: i64 = 1;
/// Where the Worker answers unless SK8_MATCH_URL or --net-match-url says otherwise (matchmaking/README.md).
pub const DEFAULT_URL: &str = "https://skate-room.sokacheskiraul.workers.dev";
pub const STUN_SERVERS: [&str; 2] = ["stun.cloudflare.com:3478", "stun.l.google.com:19302"];

pub enum Job {
    /// A request to /v1/<path> with this JSON body.
    Post(&'static str, Value),
    /// Names to resolve to IPv4 addresses ("host:port").
    Resolve(&'static str, Vec<String>),
}
pub enum Answer {
    /// The JSON the Worker answered (an `error` field for its refusals), or why there was none.
    Reply(&'static str, Result<Value, String>),
    Resolved(&'static str, Vec<SocketAddr>),
}

pub struct Client {
    jobs: Option<Sender<Job>>,
    answers: Receiver<Answer>,
}

fn post(agent: &ureq::Agent, url: &str, path: &str, body: &Value) -> Result<Value, String> {
    let response = agent
        .post(&format!("{}/v1/{path}", url.trim_end_matches('/')))
        .set("content-type", "application/json")
        .send_string(&body.to_string());
    let text = match response {
        Ok(r) => r.into_string().map_err(|e| e.to_string())?,
        // the Worker's refusals are 4xx with {error} in the body
        Err(ureq::Error::Status(code, r)) => r
            .into_string()
            .unwrap_or_else(|_| format!("{{\"error\":\"HTTP {code}\"}}")),
        Err(e) => return Err(e.to_string()),
    };
    serde_json::from_str(&text).map_err(|_| "the matchmaking service gave no answer".into())
}

fn resolve(names: &[String]) -> Vec<SocketAddr> {
    names
        .iter()
        .filter_map(|n| n.to_socket_addrs().ok())
        .flat_map(|a| a.filter(SocketAddr::is_ipv4).take(1))
        .collect()
}

impl Client {
    pub fn new(url: String) -> Self {
        let (jobs, inbox) = channel::<Job>();
        let (outbox, answers) = channel();
        std::thread::Builder::new()
            .name("rooms-http".into())
            .spawn(move || {
                let agent = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(5))
                    .timeout(Duration::from_secs(10))
                    .build();
                // ends once the Client is dropped and what it asked for is done (its leave above all)
                for job in inbox {
                    let answer = match job {
                        Job::Post(path, body) => Answer::Reply(path, post(&agent, &url, path, &body)),
                        Job::Resolve(what, names) => Answer::Resolved(what, resolve(&names)),
                    };
                    let _ = outbox.send(answer); // (nobody listening after a drop: the jobs still run)
                }
            })
            .expect("rooms thread");
        Self {
            jobs: Some(jobs),
            answers,
        }
    }
    pub fn post(&self, path: &'static str, body: Value) {
        if let Some(j) = &self.jobs {
            let _ = j.send(Job::Post(path, body));
        }
    }
    pub fn resolve(&self, what: &'static str, names: Vec<String>) {
        if let Some(j) = &self.jobs {
            let _ = j.send(Job::Resolve(what, names));
        }
    }
    pub fn answers(&self) -> Vec<Answer> {
        self.answers.try_iter().collect()
    }
}

/// The session id both ends of a room use (skate-net's packets carry it): from the room code.
pub fn session(code: &str) -> u64 {
    crate::hash(format!("skate-room:{}", code.to_ascii_uppercase()).as_bytes()) | 1
}

/// A room code as a player types it: letters and digits, upper case, at most 6.
pub fn clean_code(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .take(6)
        .collect()
}

/// The Worker's error text in words a player can act on.
pub fn explain(error: &str) -> String {
    match error {
        "no room" => "No room with that code (it may have closed)".into(),
        "full" => "That room is full".into(),
        "version" => "That room runs another version of the game".into(),
        "busy" => "The matchmaking service is busy; try again in a minute".into(),
        "old" => "Update the game: online rooms need the newest version".into(),
        other => format!("Matchmaking: {other}"),
    }
}

pub fn host_body(name: &str, addrs: &[String], max: u32) -> Value {
    json!({"v": VERSION, "rel": RELEASE, "name": name, "addrs": addrs, "max": max})
}
pub fn join_body(code: &str, name: &str, addrs: &[String]) -> Value {
    json!({"v": VERSION, "rel": RELEASE, "code": code, "name": name, "addrs": addrs})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes() {
        assert_eq!(clean_code(" ab-c2 34x9"), "ABC234");
        assert_eq!(session("abc234"), session("ABC234"));
        assert_ne!(session("ABC234"), session("ABC235"));
    }
}
