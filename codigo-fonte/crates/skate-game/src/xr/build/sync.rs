//! Online rooms (`multiplayer::rooms`): the host's park is the room's. The host publishes its map, barrier and
//! placed pieces relative to its park; guests mirror them onto their own park, wherever they put it in their room.
//! Skater states travel relative to the park too (`multiplayer::SharedFrame`, kept here), so everyone skates the
//! same layout together, and "park in front of me" on either side keeps them together. A guest's own layout is
//! kept aside (never saved over) and comes back when they leave the room; guests do not build meanwhile.
use super::park::Park;
use super::pieces::Piece;
use super::{BuildMode, Placed, PlacedPieces};
use crate::multiplayer::{Multiplayer, PARK_KEY, SharedFrame};
use crate::xr::menu::XrMenu;
use crate::xr::rig::{Rig, heading};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Pieces per record (a record holds 1 KB; a piece is about 70 bytes of JSON).
const CHUNK: usize = 10;

#[derive(Serialize, Deserialize)]
struct Header {
    rev: u64,
    map: usize,
    fence: bool,
    chunks: usize,
}
#[derive(Serialize, Deserialize)]
struct Chunk {
    rev: u64,
    pieces: Vec<Shared>,
}
/// A piece relative to the park: the piece, its base point (x, y, z) and its turn, game metres and radians.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
struct Shared(Piece, f32, f32, f32, f32);

/// A guest's own layout, while it mirrors a host's.
pub(super) struct Kept {
    items: Vec<Placed>,
    map: usize,
    fence: bool,
    park: Option<Park>,
}

fn rigid(t: &Transform) -> Mat4 {
    Mat4::from_rotation_translation(t.rotation, t.translation)
}

fn round(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

/// The placed pieces relative to the park.
fn share(items: &[Placed], park: &Transform, root: &Transform) -> Vec<Shared> {
    let into_park = rigid(park).inverse();
    items
        .iter()
        .map(|p| {
            let (_, turn, at) = (into_park * rigid(&p.world_transform(root))).to_scale_rotation_translation();
            Shared(p.piece, round(at.x), round(at.y), round(at.z), round(heading(turn)))
        })
        .collect()
}

/// Shared pieces placed on this player's park, in their room (tracking space).
fn place(shared: &[Shared], park: &Transform, root: &Transform) -> Vec<Placed> {
    let from_park = rigid(park);
    let into_room = Mat4::from_scale_rotation_translation(root.scale, root.rotation, root.translation).inverse();
    shared
        .iter()
        .enumerate()
        .map(|(i, &Shared(piece, x, y, z, turn))| {
            let world = from_park * Mat4::from_rotation_translation(Quat::from_rotation_y(turn), Vec3::new(x, y, z));
            let (_, rotation, at) = world.to_scale_rotation_translation();
            Placed {
                id: i as u64 + 1,
                piece,
                position: into_room.transform_point3(at).to_array(),
                yaw: heading(root.rotation.inverse() * rotation),
            }
        })
        .collect()
}

/// The park's frame is what the session's skater states are relative to.
pub(super) fn share_frame(rig: Res<Rig>, placed: Res<PlacedPieces>, frame: Option<ResMut<SharedFrame>>) {
    if let Some(mut frame) = frame {
        let now = placed.park.and_then(|p| p.world_transform(&rig));
        if frame.0 != now {
            frame.0 = now;
        }
    }
}

/// The host publishes its park and pieces whenever they change (and once more for every new room).
pub(super) fn publish(
    net: Option<ResMut<Multiplayer>>,
    rig: Res<Rig>,
    placed: Res<PlacedPieces>,
    mut sent: Local<Option<(u64, usize)>>,
) {
    let Some(mut net) = net else { return };
    if !net.is_host() {
        *sent = None;
        return;
    }
    let (Some(park), Some(root)) = (placed.park, rig.root_transform()) else { return };
    let Some(frame) = park.world_transform(&rig) else { return };
    if placed.busy() || sent.is_some_and(|(rev, _)| rev == placed.revision) {
        return;
    }
    let pieces = share(&placed.items, &frame, &root);
    let chunks: Vec<&[Shared]> = pieces.chunks(CHUNK).collect();
    let rev = placed.revision;
    for (i, c) in chunks.iter().enumerate() {
        let chunk = Chunk { rev, pieces: c.to_vec() };
        net.publish_record(&format!("{PARK_KEY}/{i}"), serde_json::to_vec(&chunk).unwrap_or_default());
    }
    // chunks the layout no longer needs are withdrawn
    for i in chunks.len()..sent.map_or(0, |(_, n)| n) {
        net.publish_record(&format!("{PARK_KEY}/{i}"), vec![]);
    }
    let header = Header { rev, map: park.map, fence: park.fence, chunks: chunks.len() };
    net.publish_record(PARK_KEY, serde_json::to_vec(&header).unwrap_or_default());
    info!("ONLINE park shared: map {} and {} pieces (revision {rev})", park.map, pieces.len());
    *sent = Some((rev, chunks.len()));
}

/// A guest mirrors the host's park onto their own, and gets their own layout back on leaving.
#[allow(clippy::too_many_arguments)]
pub(super) fn mirror(
    net: Option<Res<Multiplayer>>,
    rig: Res<Rig>,
    menu: Res<XrMenu>,
    mut placed: ResMut<PlacedPieces>,
    mut mode: ResMut<BuildMode>,
    mut applied: Local<Option<(u64, Park)>>,
) {
    let records = net.as_ref().filter(|n| n.active() && !n.is_host()).and_then(|n| n.host_park());
    let Some(records) = records else {
        if let Some(kept) = placed.kept.take() {
            // left the room: their own map, barrier and pieces again (the park stays where it is now)
            placed.items = kept.items;
            placed.map_choice = kept.map;
            let park = match placed.park {
                Some(p) => Some(Park { map: kept.map, fence: kept.fence, ..p }),
                None => kept.park,
            };
            placed.set_park(park);
            info!("ONLINE own park back");
        }
        *applied = None;
        return;
    };
    let read = |key: &str| records.get(key).and_then(|v| serde_json::from_slice(v).ok());
    let Some(header) = read(PARK_KEY).and_then(|v: serde_json::Value| serde_json::from_value::<Header>(v).ok())
    else {
        return;
    };
    let mut shared = vec![];
    for i in 0..header.chunks {
        let chunk = read(&format!("{PARK_KEY}/{i}")).and_then(|v| serde_json::from_value::<Chunk>(v).ok());
        match chunk {
            Some(c) if c.rev == header.rev => shared.extend(c.pieces),
            _ => return, // (the rest of this revision is on its way)
        }
    }
    if mode.active {
        mode.active = false;
    }
    if menu.moving_park || placed.busy() {
        return;
    }
    if placed.kept.is_none() {
        let (map, fence) = placed.park.map_or((placed.map_choice, true), |p| (p.map, p.fence));
        placed.kept = Some(Kept { items: placed.items.clone(), map, fence, park: placed.park });
    }
    placed.map_choice = header.map;
    let Some(park) = placed.park else {
        // no park yet: one in front of them, with the host's map; the pieces follow once it is down
        placed.place_in_front = true;
        return;
    };
    if park.map != header.map || park.fence != header.fence {
        placed.set_park(Some(Park { map: header.map, fence: header.fence, ..park }));
        return;
    }
    if applied.is_some_and(|(rev, at)| rev == header.rev && at == park) {
        return;
    }
    let (Some(frame), Some(root)) = (park.world_transform(&rig), rig.root_transform()) else { return };
    placed.items = place(&shared, &frame, &root);
    placed.revision += 1;
    *applied = Some((header.rev, park));
    info!("ONLINE mirroring the host's park: map {} and {} pieces", header.map, shared.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xr::build::pieces::PieceKind;

    /// A piece shared from one room arrives at the same spot on the other player's park, though the two parks
    /// stand in different places, turns and world scales.
    #[test]
    fn pieces_keep_their_place_on_the_park() {
        let piece = Piece { kind: PieceKind::Rail, size: 1.5 };
        let host_root = Transform { translation: Vec3::new(3.0, 0.5, -2.0), rotation: Quat::from_rotation_y(0.3), scale: Vec3::splat(12.0) };
        let host_park = Transform::from_xyz(10.0, 1.0, 4.0).with_rotation(Quat::from_rotation_y(1.1));
        let guest_root = Transform { translation: Vec3::new(-7.0, 0.0, 9.0), rotation: Quat::from_rotation_y(-2.0), scale: Vec3::splat(20.0) };
        let guest_park = Transform::from_xyz(-4.0, 0.8, 30.0).with_rotation(Quat::from_rotation_y(2.4));
        let mine = vec![Placed { id: 7, piece, position: [0.4, 0.9, -0.3], yaw: 0.7 }];
        let shared = share(&mine, &host_park, &host_root);
        let theirs = place(&shared, &guest_park, &guest_root);
        // the same place relative to each park, in game metres
        let rel = |p: &Placed, root: &Transform, park: &Transform| {
            let w = p.world_transform(root);
            (park.rotation.inverse() * (w.translation - park.translation), heading(park.rotation.inverse() * w.rotation))
        };
        let (a, ta) = rel(&mine[0], &host_root, &host_park);
        let (b, tb) = rel(&theirs[0], &guest_root, &guest_park);
        assert!(a.distance(b) < 0.01, "{a} vs {b}");
        assert!((ta - tb).abs() < 0.01);
        assert_eq!(theirs[0].piece, piece);
        // and it fits a record
        let chunk = Chunk { rev: 1, pieces: vec![shared[0]; CHUNK] };
        assert!(serde_json::to_vec(&chunk).unwrap().len() < skate_net::lobby::MAX_APP_VALUE);
    }
}
