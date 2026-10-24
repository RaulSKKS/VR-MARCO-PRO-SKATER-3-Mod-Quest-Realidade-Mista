//! Transport-neutral ten-player free-skate; each player owns their simulation.
pub(crate) mod appearance;
mod appearance_transfer;
mod render;
mod rooms;
mod transport;
pub(crate) use render::RemoteActor;
use crate::{
    app::{FrameSet, SimulationSet},
    physics::{GamePhysics, SkaterRuntime, network},
};
use bevy::prelude::*;
use skate_net::{
    directory::{Command as LobbyCommand, Event, Row},
    lobby::{Info, Session},
    packed::{self, BodyState, Packed},
};
use std::{
    collections::{BTreeMap, VecDeque},
    net::SocketAddr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
/// The session record each player publishes their name in (the host forwards it to everyone, late joiners too).
const NAME_KEY: &str = "@name";
/// Records only an online room's host publishes: its Marco park and placed pieces, which guests mirror.
pub(crate) const PARK_KEY: &str = "@park";
/// Each player's Marco outfit (one byte: the skin), so others dress their skater the same.
pub(crate) const SKIN_KEY: &str = "@marco";
/// Each player's points (total and best line, two little-endian i64), for the park's big-screen ranking.
pub(crate) const SCORE_KEY: &str = "@score";

/// Session records the game keeps for itself (Lua mods neither see nor publish them).
fn reserved(key: &str) -> bool {
    key.starts_with("@look/") || key == NAME_KEY || key.starts_with(PARK_KEY) || key == SKIN_KEY || key == SCORE_KEY
}

/// The frame both ends of a session agree on: the Marco park's, in the game world. Skater states go out relative
/// to it and come in placed on this player's own park, so skaters meet on the park wherever each player put it in
/// their room. None (no park, or no XR): states go as they are.
#[derive(Resource, Default)]
pub(crate) struct SharedFrame(pub Option<Transform>);
impl SharedFrame {
    /// The matrix taking world states out to the shared frame (`out`) or back in.
    fn matrix(&self, out: bool) -> Option<Mat4> {
        let m = self.0.map(|t| Mat4::from_rotation_translation(t.rotation, t.translation))?;
        Some(if out { m.inverse() } else { m })
    }
}
fn reframe_body(state: &mut BodyState, m: Mat4) {
    state.root = network::pose(m * network::matrix(state.root));
    for b in &mut state.bodies {
        b.pose = network::pose(m * network::matrix(b.pose));
        b.velocity = m.transform_vector3(Vec3::from_array(b.velocity)).to_array();
        b.angular = m.transform_vector3(Vec3::from_array(b.angular)).to_array();
    }
}
const NAME_FILE: &str = "marco-name.txt";

/// A player name as shown: printable ASCII, at most 16 characters, never empty.
pub(crate) fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(16).collect();
    let name = name.trim();
    if name.is_empty() { "Skater".into() } else { name.into() }
}

pub(crate) fn unique() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    skate_net::hash(
        &[
            nanos.to_le_bytes().as_slice(),
            std::process::id().to_le_bytes().as_slice(),
        ]
        .concat(),
    )
}
#[derive(Default)]
pub(crate) struct Options {
    pub direct: Option<(SocketAddr, SocketAddr)>,
    pub host: Option<SocketAddr>,
    pub session: u64,
    pub spawn_offset: f32,
    pub appearance: Option<String>,
    pub title: Option<String>,
    pub controller: Option<u32>,
    /// --net-room-host [players] / --net-room-join CODE: an online room through the matchmaking service.
    pub room_host: Option<u32>,
    pub room_join: Option<String>,
    pub match_url: Option<String>,
    pub name: Option<String>,
}
struct VisualRoot {
    captured: f64,
    received: f64,
    pose: skate_net::Pose,
}
struct VisualPose {
    captured: f64,
    received: f64,
    bones: Vec<skate_net::Bone>,
}
struct Remote {
    roots: VecDeque<VisualRoot>,
    poses: VecDeque<VisualPose>,
    epoch: u64,
    visual_since: u64,
    body: BodyState,
    body_at: Instant,
    body_seq: u32,
    pose_seq: u32,
}
#[derive(Resource)]
pub(crate) struct Multiplayer {
    transport: Option<Box<dyn transport::Transport>>,
    lobby: Option<Session>,
    info: Info,
    schema: network::Schema,
    anchors: Vec<usize>,
    remotes: BTreeMap<u64, Remote>,
    pub status: String,
    pub join_code: String,
    pub host_code: String,
    last_body: Instant,
    last_pose: Instant,
    started: Instant,
    last_metrics: Instant,
    counts: (u64, u64),
    rates: String,
    provider_metrics: String,
    visual_status: String,
    loopback: bool,
    room: Option<(u64, u64)>,
    map_name: String,
    pub browser_rows: Vec<Row>,
    pub browser_page: usize,
    pub browser_total: usize,
    pub browser_status: String,
    match_url: String,
    player_name: String,
    name_file: Option<std::path::PathBuf>,
}
impl Multiplayer {
    pub(crate) fn diagnostic_summary(&self) -> String {
        let provider = if self.room.is_some() {
            "platform_relay"
        } else if self.transport.is_some() {
            "direct_local"
        } else {
            "inactive"
        };
        let rtt = self.lobby.as_ref().map(|lobby| lobby.stats.rtt_ms);
        format!(
            "provider:{provider} active:{} remote_count:{} rtt_ms:{rtt:?}",
            self.active(),
            self.remotes.len()
        )
    }
    pub(crate) fn mod_identity(&self) -> (bool, u64, bool) {
        self.lobby
            .as_ref()
            .map_or((false, 0, true), |l| (true, l.local, l.is_host()))
    }
    pub(crate) fn collision_players(&self) -> Vec<(u64, skate_net::packed::BodyState)> {
        self.remotes
            .iter()
            .filter(|(_, r)| r.body_at.elapsed() < Duration::from_millis(500))
            .map(|(&id, r)| (id, r.body.clone()))
            .collect()
    }
    pub(crate) fn mod_records(&self) -> Vec<(u64, String, u32, Vec<u8>)> {
        self.lobby.as_ref().map_or_else(Vec::new, |l| {
            l.actors
                .iter()
                .filter(|(id, _)| **id != l.local)
                .flat_map(|(&id, actor)| {
                    actor
                        .application
                        .iter()
                        .filter(|(key, _)| !reserved(key))
                        .map(move |(key, r)| (id, key.clone(), r.seq, r.value.clone()))
                })
                .collect()
        })
    }
    pub(crate) fn publish_mod(&mut self, key: &str, value: Vec<u8>) -> bool {
        if reserved(key) {
            return false;
        }
        let now = self.started.elapsed().as_millis() as u64;
        self.lobby
            .as_mut()
            .is_some_and(|l| l.publish_application(key, value, now))
    }
    pub fn active(&self) -> bool {
        self.lobby.is_some()
    }
    /// The name other players see.
    pub fn name(&self) -> &str {
        &self.player_name
    }
    /// Changes it (and remembers it for the next launch); a room under way shows it at once.
    pub fn set_name(&mut self, name: &str) {
        self.player_name = clean_name(name);
        if let Some(path) = &self.name_file {
            if let Err(e) = std::fs::write(path, &self.player_name) {
                warn!("MULTIPLAYER could not save the name: {e}");
            }
        }
        info!("MULTIPLAYER name {}", self.player_name);
    }
    /// A remote skater's name, once their record has arrived.
    pub(crate) fn name_of(&self, actor: u64) -> Option<String> {
        let record = self.lobby.as_ref()?.actors.get(&actor)?.application.get(NAME_KEY)?;
        Some(clean_name(&String::from_utf8_lossy(&record.value)))
    }
    /// This player hosts the session (an online room's maker).
    pub(crate) fn is_host(&self) -> bool {
        self.lobby.as_ref().is_some_and(Session::is_host)
    }
    /// A remote skater's record (their outfit, say), once it has arrived.
    pub(crate) fn record_of(&self, actor: u64, key: &str) -> Option<&[u8]> {
        let record = self.lobby.as_ref()?.actors.get(&actor)?.application.get(key)?;
        (!record.value.is_empty()).then_some(record.value.as_slice())
    }
    /// Publishes one of this player's own records (empty: withdrawn); unchanged values cost nothing.
    pub(crate) fn publish_record(&mut self, key: &str, value: Vec<u8>) {
        let now = self.started.elapsed().as_millis() as u64;
        if let Some(lobby) = &mut self.lobby {
            lobby.publish_application(key, value, now);
        }
    }
    /// The host's park records as this guest has them (key -> value), from whichever skater publishes them.
    pub(crate) fn host_park(&self) -> Option<BTreeMap<String, Vec<u8>>> {
        let lobby = self.lobby.as_ref().filter(|l| !l.is_host())?;
        let host = lobby
            .actors
            .iter()
            .find(|(id, a)| **id != lobby.local && a.application.contains_key(PARK_KEY))?
            .1;
        Some(
            host.application
                .iter()
                .filter(|(k, r)| k.starts_with(PARK_KEY) && !r.value.is_empty())
                .map(|(k, r)| (k.clone(), r.value.clone()))
                .collect(),
        )
    }
    /// Everyone in the session, this player first: (actor, name, is this player).
    pub(crate) fn names(&self) -> Vec<(u64, String, bool)> {
        let Some(lobby) = &self.lobby else {
            return vec![];
        };
        let mut out = vec![(lobby.local, self.player_name.clone(), true)];
        for &id in lobby.actors.keys().filter(|&&id| id != lobby.local) {
            out.push((id, self.name_of(id).unwrap_or_else(|| "...".into()), false));
        }
        out
    }
    pub fn leave(&mut self) {
        if let (Some(lobby), Some(t)) = (&self.lobby, &mut self.transport) {
            for p in lobby.goodbye() {
                let _ = t.send(p.peer, &p.data);
            }
        }
        self.transport = None;
        self.lobby = None;
        self.remotes.clear();
        self.host_code.clear();
        self.room = None;
        self.status = "Offline. Steam is only needed for Steam multiplayer.".into();
    }
    fn start(&mut self, transport: Box<dyn transport::Transport>, session: u64, host: Option<u64>) {
        self.leave();
        self.info.id = unique();
        self.loopback = transport.loopback();
        let mut lobby = Session::new(session, self.info, host);
        lobby.set_loopback(self.loopback);
        self.lobby = Some(lobby);
        self.transport = Some(transport);
        self.started = Instant::now();
        self.last_metrics = Instant::now();
        self.counts = (0, 0);
        self.rates.clear();
        self.provider_metrics.clear();
        self.visual_status.clear();
        self.status = "Waiting for players... (up to 10)".into();
    }
    pub fn local(&mut self, host: bool) {
        let bind = if host {
            "127.0.0.1:31030"
        } else {
            "127.0.0.1:0"
        };
        match transport::Direct::new(bind.parse().unwrap()) {
            Ok(t) => self.start(
                Box::new(t),
                480_31030,
                if host {
                    None
                } else {
                    Some(transport::endpoint("127.0.0.1:31030".parse().unwrap()).unwrap())
                },
            ),
            Err(e) => self.status = format!("Could not open local session: {e}"),
        }
    }
    /// Makes an online room for up to `max` skaters; its code shows in `host_code` once the service gives it.
    pub fn room_host(&mut self, max: u32) {
        self.room_start(rooms::Start::Host { max }, None);
    }
    /// Joins the online room with this code.
    pub fn room_join(&mut self, code: &str) {
        let code = skate_net::rooms::clean_code(code);
        let session = skate_net::rooms::session(&code);
        self.room_start(rooms::Start::Join(code), Some(session));
    }
    fn room_start(&mut self, start: rooms::Start, session: Option<u64>) {
        match rooms::Rooms::new(self.match_url.clone(), start, &self.player_name) {
            // a host's session id comes from its code, which the service has yet to give (see `receive`)
            Ok(t) => self.start(Box::new(t), session.unwrap_or(0), session.map(|_| rooms::HOST)),
            Err(e) => {
                self.leave();
                self.status = e;
            }
        }
    }
    /// The room code to tell friends (an online room's host), if there is one.
    pub fn room_code(&self) -> Option<&str> {
        let code = self.host_code.as_str();
        (code.len() == 6 && self.room.is_none() && self.lobby.as_ref().is_some_and(Session::is_host))
            .then_some(code)
    }
    fn lobby_command(&mut self, command: LobbyCommand) {
        // Retry discovery after Steam was opened following an initialization failure.
        if !self.active()
            && self.transport.as_ref().is_some_and(|t| {
                let status = t.status();
                status.starts_with("ERROR")
                    || status.contains("stopped")
                    || status.contains("did not respond")
            })
        {
            self.transport = None;
        }
        if self.transport.is_none() {
            match transport::Steam::new(0, 0) {
                Ok(t) => self.transport = Some(Box::new(t)),
                Err(e) => {
                    self.status = e.clone();
                    self.browser_status = e;
                    return;
                }
            }
        }
        let result = self.transport.as_mut().unwrap().command(command);
        self.browser_status = match result {
            Ok(()) => "Contacting Steam...".into(),
            Err(e) => e,
        };
        if !self.active() {
            self.status = self.browser_status.clone();
        }
    }
    pub fn browse(&mut self, page: usize) {
        self.lobby_command(LobbyCommand::Browse {
            page,
            map: self.info.map,
            physics: self.info.physics,
        });
    }
    pub fn join_row(&mut self, index: usize) {
        let Some(row) = self.browser_rows.get(index) else {
            return;
        };
        if row.players >= row.capacity {
            self.browser_status = "Lobby is full. Refresh to check for a free slot.".into();
            return;
        }
        let lobby = row.id;
        self.leave();
        self.lobby_command(LobbyCommand::Join {
            lobby,
            map: self.info.map,
            physics: self.info.physics,
        });
    }
    pub fn steam(&mut self, host: bool) {
        if host {
            self.leave();
            self.lobby_command(LobbyCommand::Host {
                map: self.info.map,
                physics: self.info.physics,
                label: self.map_name.clone(),
            });
            return;
        }
        if let Ok(lobby) = self.join_code.trim().parse::<u64>() {
            if lobby != 0 {
                self.leave();
                self.lobby_command(LobbyCommand::Join {
                    lobby,
                    map: self.info.map,
                    physics: self.info.physics,
                });
                return;
            }
        }
        let (peer, session) = {
            let Some((id, key)) = self.join_code.trim().split_once('-') else {
                self.status = "Enter a lobby code, or select a lobby in the browser.".into();
                return;
            };
            match (id.parse::<u64>(), u64::from_str_radix(key, 16)) {
                (Ok(id), Ok(key)) if id != 0 && key != 0 => (id, key),
                _ => {
                    self.status = "Invalid join code. Expected SteamID-session.".into();
                    return;
                }
            }
        };
        match transport::Steam::new(peer, session) {
            Ok(t) => self.start(Box::new(t), session, (peer != 0).then_some(peer)),
            Err(e) => self.status = e,
        }
    }
}
pub(crate) struct MultiplayerPlugin;
impl Plugin for MultiplayerPlugin {
    fn build(&self, app: &mut App) {
        let config = app.world().resource::<crate::config::Config>();
        let skater = app.world().resource::<SkaterRuntime>();
        let physics = app.world().resource::<GamePhysics>();
        let rig = skate_net::hash(
            format!(
                "{:?}{:?}",
                skater.animation.evaluator.frames.bone_names,
                skater.animation.evaluator.frames.parents
            )
            .as_bytes(),
        );
        let schema =
            network::Schema::new(physics, skater).expect("Default multiplayer collision schema");
        let mut net = Multiplayer {
            transport: None,
            lobby: None,
            info: Info {
                id: unique(),
                map: config.map_fingerprint,
                rig,
                physics: schema.fingerprint,
                appearance: skate_net::hash(
                    config
                        .multiplayer
                        .appearance
                        .as_deref()
                        .unwrap_or(skate_net::DEFAULT_APPEARANCE)
                        .as_bytes(),
                ),
            },
            schema,
            anchors: network::anchors(skater),
            remotes: BTreeMap::new(),
            status: "Offline. Steam is only needed for Steam multiplayer.".into(),
            join_code: String::new(),
            host_code: String::new(),
            last_body: Instant::now(),
            last_pose: Instant::now(),
            started: Instant::now(),
            last_metrics: Instant::now(),
            counts: (0, 0),
            rates: String::new(),
            provider_metrics: String::new(),
            visual_status: String::new(),
            loopback: false,
            room: None,
            map_name: config
                .map_path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|n| skate_net::directory::label(&n.to_string_lossy()))
                .unwrap_or_else(|| "Test world".into()),
            browser_rows: vec![],
            browser_page: 0,
            browser_total: 0,
            browser_status: String::new(),
            match_url: rooms::match_url(config.multiplayer.match_url.as_deref()),
            // --net-name, else the name chosen in the online menu (kept beside the game data), else the account's
            player_name: clean_name(
                &config
                    .multiplayer
                    .name
                    .clone()
                    .or_else(|| std::fs::read_to_string(name_file(config)).ok())
                    .or_else(|| std::env::var("USERNAME").ok())
                    .or_else(|| std::env::var("USER").ok())
                    .unwrap_or_default(),
            ),
            name_file: Some(name_file(config)),
        };
        if let Some(bind) = config
            .multiplayer
            .host
            .or(config.multiplayer.direct.map(|(b, _)| b))
        {
            let target = config
                .multiplayer
                .direct
                .map(|(_, p)| transport::endpoint(p).expect("IPv4 peer"));
            match transport::Direct::new(bind) {
                Ok(t) => net.start(Box::new(t), config.multiplayer.session, target),
                Err(e) => net.status = format!("Local multiplayer could not start: {e}"),
            }
        }
        if let Some(max) = config.multiplayer.room_host {
            net.room_host(max);
        } else if let Some(code) = config.multiplayer.room_join.clone() {
            net.room_join(&code);
        }
        app.insert_resource(net)
            .add_systems(
                PreUpdate,
                (world_changed, receive)
                    .chain()
                    .after(crate::map_transition::MapTransitionSet),
            )
            .add_systems(Startup, setup_hud)
            .add_systems(Update, hud)
            .add_systems(Update, send_pose.after(crate::modding::vehicles::present))
            .add_systems(
                FixedUpdate,
                prepare
                    .before(SimulationSet::Physics)
                    .after(SimulationSet::Controls),
            )
            .add_systems(FixedUpdate, send.after(SimulationSet::Physics))
            .init_resource::<appearance::Appearances>()
            .init_resource::<SharedFrame>()
            .add_systems(Last, appearance::cleanup)
            .add_systems(Update, appearance::sync.before(render::spawn))
            .add_plugins(render::RemoteRenderPlugin);
    }
}
fn name_file(config: &crate::config::Config) -> std::path::PathBuf {
    config.asset_root.parent().unwrap_or(&config.asset_root).join(NAME_FILE)
}
// A world swap replaces the local physics assembly. Never retain lobby identity
// or remote collision state from the previous map, including pending Steam joins.
fn world_changed(
    mut changed: MessageReader<crate::map_transition::WorldChanged>,
    config: Res<crate::config::Config>,
    physics: Res<GamePhysics>,
    skater: Res<SkaterRuntime>,
    mut net: ResMut<Multiplayer>,
) {
    if changed.read().count() == 0 {
        return;
    }
    net.leave();
    net.info.map = config.map_fingerprint;
    net.map_name = config
        .map_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|n| skate_net::directory::label(&n.to_string_lossy()))
        .unwrap_or_else(|| "Test world".into());
    if let Ok(schema) = network::Schema::new(&physics, &skater) {
        net.info.physics = schema.fingerprint;
        net.schema = schema;
    }
    net.anchors = network::anchors(&skater);
    net.browser_rows.clear();
    net.browser_page = 0;
    net.browser_total = 0;
    net.browser_status.clear();
}
fn receive(mut net: ResMut<Multiplayer>, frame: Res<SharedFrame>) {
    let frame = frame.matrix(false);
    let into_world = |mut state: BodyState| {
        if let Some(m) = frame {
            reframe_body(&mut state, m);
        }
        state
    };
    let now = net.started.elapsed().as_millis() as u64;
    let net = &mut *net;
    let Some(t) = &mut net.transport else {
        return;
    };
    let packets = match t.receive() {
        Ok(p) => p,
        Err(e) => {
            net.status = format!("Connection error: {e}");
            return;
        }
    };
    let provider = t.status();
    for response in t.events() {
        match response.event {
            Event::Rows { page, total, rows } => {
                net.browser_rows = rows;
                net.browser_page = page;
                net.browser_total = total;
                net.browser_status = if total == 0 {
                    "No public lobbies found. Host one or refresh.".into()
                } else {
                    format!(
                        "{} lobbies | Page {} | select a row to join",
                        total,
                        page + 1
                    )
                };
            }
            Event::Error(e) => {
                net.browser_status = e.clone();
                if net.lobby.is_none() {
                    net.status = e;
                }
            }
            Event::Owner {
                lobby: room,
                owner,
                own,
            } => {
                if net.room != Some((room, owner)) {
                    let host = (owner != own).then_some(owner);
                    if net.room.is_some_and(|(id, _)| id == room) {
                        if let Some(session) = &mut net.lobby {
                            session.migrate(host, now);
                        }
                        info!(
                            "MULTIPLAYER_HOST_MIGRATED lobby={room} owner={owner} local_host={}",
                            host.is_none()
                        );
                    } else {
                        net.info.id = unique();
                        net.lobby = Some(Session::new(room, net.info, host));
                        net.counts = (0, 0);
                        net.last_metrics = Instant::now();
                    }
                    net.remotes.clear();
                    net.loopback = false;
                    net.room = Some((room, owner));
                    net.host_code = room.to_string();
                    net.browser_status = if host.is_none() {
                        "Hosting public lobby. Other players can join from the browser.".into()
                    } else {
                        "Lobby joined; connecting to host...".into()
                    };
                }
            }
        }
    }
    let Some(lobby) = &mut net.lobby else {
        if provider.starts_with("ERROR")
            || provider.contains("stopped")
            || provider.contains("did not respond")
        {
            net.browser_status = provider.clone();
            net.status = provider;
        }
        return;
    };
    if net.room.is_none() && lobby.is_host() {
        if let Some(id) = provider.strip_prefix("READY ") {
            net.host_code = format!("{}-{:016x}", id, lobby.session);
        }
        // an online room: its code names the session, and is what the host tells friends
        if let Some(code) = t.room_code() {
            if net.host_code != code {
                lobby.session = skate_net::rooms::session(&code);
                info!("MULTIPLAYER_ROOM code={code}");
                net.host_code = code;
            }
        }
    }
    if t.failed() {
        net.status = provider;
        return;
    }
    // (unchanged values are not sent again: this costs nothing once published)
    lobby.publish_application(NAME_KEY, net.player_name.as_bytes().to_vec(), now);
    lobby.set_congested(t.congested());
    net.provider_metrics = t.metrics();
    for (peer, p) in packets {
        lobby.receive(peer, &p, now);
    }
    for p in lobby.service(now) {
        let success = t.send(p.peer, &p.data).is_ok();
        lobby.record_send(p.data.len(), success);
    }
    net.remotes.retain(|id, _| lobby.actors.contains_key(id));
    for (&id, actor) in &lobby.actors {
        if id == lobby.local {
            continue;
        }
        let Some(body) = actor.body.latest() else {
            continue;
        };
        if now.saturating_sub(body.received) > 3500 {
            net.remotes.remove(&id);
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = net.remotes.entry(id) {
            let Some(initial) = body.state.unpack_body().map(into_world) else {
                continue;
            };
            let fallback = actor.info.rig != net.info.rig;
            info!("MULTIPLAYER_CONNECTED peer={id} fallback={fallback}");
            entry.insert(Remote {
                roots: VecDeque::new(),
                poses: VecDeque::new(),
                epoch: 0,
                visual_since: 0,
                body: initial,
                body_at: Instant::now(),
                body_seq: 0,
                pose_seq: 0,
            });
        }
        let remote = net.remotes.get_mut(&id).unwrap();
        // Consume every newly decoded source sample, including several delivered in one frame.
        for revision in &actor.body.history {
            if revision.seq <= remote.body_seq {
                continue;
            }
            let Some(state) = revision.state.unpack_body().map(into_world) else {
                continue;
            };
            if remote.roots.back().is_some_and(|p| {
                Vec3::from_array(p.pose.p).distance(Vec3::from_array(state.root.p)) > 10.
            }) {
                remote.roots.clear();
                remote.poses.clear();
                remote.epoch += 1;
                remote.visual_since = revision.state.captured;
            }
            remote.roots.push_back(VisualRoot {
                captured: revision.state.captured as f64 / 1000.,
                received: revision.received as f64 / 1000.,
                pose: state.root,
            });
            while remote.roots.len() > 64 {
                remote.roots.pop_front();
            }
            remote.body = state;
            remote.body_at =
                Instant::now() - Duration::from_millis(now.saturating_sub(revision.received));
            remote.body_seq = revision.seq;
        }
        for revision in &actor.pose.history {
            if revision.seq <= remote.pose_seq {
                continue;
            }
            remote.pose_seq = revision.seq;
            if revision.state.captured < remote.visual_since {
                continue;
            }
            let Some(mut pose) = revision.state.unpack_pose() else {
                continue;
            };
            if actor.info.rig != net.info.rig
                || pose
                    .bones
                    .iter()
                    .map(|b| b.index as usize)
                    .ne(net.anchors.iter().copied())
            {
                pose.bones.clear();
            }
            remote.poses.push_back(VisualPose {
                captured: revision.state.captured as f64 / 1000.,
                received: revision.received as f64 / 1000.,
                bones: pose.bones,
            });
            while remote.poses.len() > 64 {
                remote.poses.pop_front();
            }
        }
    }
    net.status = if !lobby.notice.is_empty() {
        lobby.notice.clone()
    } else if lobby.actors.len() > 1 {
        format!(
            "Connected: {}/10 players | collisions on | synced characters",
            lobby.actors.len()
        )
    } else {
        format!("{} | waiting for players (1/10)", provider)
    };
    if net.last_metrics.elapsed() >= Duration::from_secs(1) {
        let dt = net.last_metrics.elapsed().as_secs_f64();
        let s = &lobby.stats;
        net.rates = format!(
            "App up {:.1} / down {:.1} kB/s | RTT {} ms | stale {} | delta misses {} | send errors {}",
            (s.tx_bytes - net.counts.0) as f64 / dt / 1000.,
            (s.rx_bytes - net.counts.1) as f64 / dt / 1000.,
            s.rtt_ms,
            s.late,
            s.baseline_miss,
            s.send_errors
        );
        info!(
            "MULTIPLAYER_STATS players={} {} budget_skips={} {}",
            lobby.actors.len(),
            net.rates,
            s.budget_skips,
            net.provider_metrics
        );
        net.counts = (s.tx_bytes, s.rx_bytes);
        net.last_metrics = Instant::now();
    }
}
fn prepare(
    net: Res<Multiplayer>,
    mut physics: ResMut<GamePhysics>,
    skater: Res<SkaterRuntime>,
    vehicles: Res<crate::modding::vehicles::Vehicles>,
) {
    physics.network_active = net.active();
    physics.network_contacts = 0;
    let mut proxies = std::mem::take(&mut physics.network_proxies);
    proxies.bodies.clear();
    proxies.volumes.clear();
    for remote in net.remotes.values() {
        if remote.body_at.elapsed() < Duration::from_millis(500) {
            proxies.append(
                &remote.body,
                &net.schema,
                &physics,
                &skater,
                remote.body_at.elapsed().as_secs_f32().min(0.05),
            );
        }
    }
    for shape in crate::modding::vehicles::network::collision_shapes(&vehicles) {
        proxies.append_vehicle(&shape, &physics, &skater);
    }
    physics.network_proxies = proxies;
}
#[derive(Component)]
struct NetworkHud;
fn setup_hud(mut commands: Commands) {
    commands.spawn((
        NetworkHud,
        Text::new(""),
        TextFont {
            font_size: 15.,
            ..default()
        },
        TextColor(Color::WHITE),
        GlobalZIndex(5),
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            padding: UiRect::all(px(6)),
            ..default()
        },
        BackgroundColor(Color::srgba(0., 0., 0., 0.6)),
    ));
}
fn hud(
    net: Res<Multiplayer>,
    physics: Res<GamePhysics>,
    mut label: Single<&mut Text, With<NetworkHud>>,
    mods: Res<crate::modding::network::ModSync>,
    appearances: Res<appearance::Appearances>,
) {
    ***label = if net.active() {
        let names: Vec<String> =
            net.names().into_iter().map(|(_, n, me)| if me { format!("{n} (you)") } else { n }).collect();
        format!(
            "{}\nSkaters: {}\n{}\n{}\n{}\n{}\n{} {}\nPlayer contacts: {} | Esc > Multiplayer",
            net.status,
            names.join(", "),
            net.rates,
            net.provider_metrics,
            net.visual_status,
            mods.status,
            appearances.progress,
            appearances.status,
            physics.network_contacts
        )
    } else {
        String::new()
    };
}
fn send(
    mut net: ResMut<Multiplayer>,
    physics: Res<GamePhysics>,
    skater: Res<SkaterRuntime>,
    vehicles: Res<crate::modding::vehicles::Vehicles>,
    frame: Res<SharedFrame>,
) {
    if !net.active() || skater.pose_generation == 0 {
        return;
    }
    let now = net.started.elapsed().as_millis() as u64;
    if net.last_body.elapsed() >= Duration::from_millis(49) {
        let mut state = network::capture_body(&physics, &skater);
        if let Some(pose) = &vehicles.network_pose {
            state.root = pose.root;
        }
        if let Some(m) = frame.matrix(true) {
            reframe_body(&mut state, m);
        }
        if vehicles.occupied() {
            state.enabled = 1u64 << 62;
        }
        if let Some(p) = Packed::body(&state) {
            net.lobby.as_mut().unwrap().publish(packed::BODY, p, now);
        }
        net.last_body = Instant::now();
    }
}
fn send_pose(
    mut net: ResMut<Multiplayer>,
    skater: Res<SkaterRuntime>,
    vehicles: Res<crate::modding::vehicles::Vehicles>,
    frame: Res<SharedFrame>,
) {
    if !net.active() || skater.pose_generation == 0 {
        return;
    }
    let now = net.started.elapsed().as_millis() as u64;
    if net.last_pose.elapsed() >= Duration::from_millis(if net.loopback { 49 } else { 99 }) {
        let mut pose = vehicles
            .network_pose
            .clone()
            .unwrap_or_else(|| network::capture_pose(&skater, &net.anchors));
        // (the bones are the skeleton's own space; only the root is in the world)
        if let Some(m) = frame.matrix(true) {
            pose.root = network::pose(m * network::matrix(pose.root));
        }
        if let Some(p) = Packed::pose(&pose) {
            net.lobby.as_mut().unwrap().publish(packed::POSE, p, now);
        }
        net.last_pose = Instant::now();
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    use skate_net::{Body, Pose};

    /// A skater sent from one player's park arrives at the same spot of the other's, wherever each park stands.
    #[test]
    fn states_travel_relative_to_the_park() {
        let host = SharedFrame(Some(Transform::from_xyz(40.0, 2.0, -15.0).with_rotation(Quat::from_rotation_y(0.8))));
        let guest = SharedFrame(Some(Transform::from_xyz(-3.0, 0.5, 60.0).with_rotation(Quat::from_rotation_y(-2.1))));
        let on_park = Vec3::new(1.5, 0.3, -2.0);
        let at = host.0.unwrap().transform_point(on_park);
        let q = (host.0.unwrap().rotation * Quat::from_rotation_y(0.4)).to_array();
        let pose = Pose { p: at.to_array(), q };
        let forward = host.0.unwrap().rotation * Vec3::NEG_Z * 5.0;
        let mut state = BodyState {
            root: pose,
            enabled: 0,
            bodies: vec![Body { pose, velocity: forward.to_array(), angular: [0.0; 3] }],
        };
        reframe_body(&mut state, host.matrix(true).unwrap());
        reframe_body(&mut state, guest.matrix(false).unwrap());
        let g = guest.0.unwrap();
        let arrived = g.rotation.inverse() * (Vec3::from_array(state.root.p) - g.translation);
        assert!(arrived.distance(on_park) < 1e-3, "{arrived}");
        let turn = g.rotation.inverse() * Quat::from_array(state.root.q);
        assert!(turn.angle_between(Quat::from_rotation_y(0.4)) < 1e-3);
        let v = g.rotation.inverse() * Vec3::from_array(state.bodies[0].velocity);
        assert!(v.distance(Vec3::NEG_Z * 5.0) < 1e-3);
        // without parks nothing changes
        assert!(SharedFrame(None).matrix(true).is_none());
    }
}
