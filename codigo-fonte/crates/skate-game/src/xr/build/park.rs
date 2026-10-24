//! The Marco park: a fixed, compact skatepark on a thick base, like a board
//! game in mixed reality. It goes in front of the player (floating, or on a
//! table or the floor), scales with the world, has an optional glass barrier
//! round its edge and the Marco VR statue. Five maps ([`MAPS`]) can be picked
//! in the menu: four from the "Skate VR Tabletop Architect" redesign (real
//! decks behind every ramp, stairs coming down from a raised deck, boxes you
//! can ride onto; laid out on its 28 x 18 m tray, centred on the deck) and
//! the closed bowl. Riding off the edge just carries on in the room.
//!
//! Its pieces are generated from the chosen map every time they are needed,
//! so they follow the world scale together; only its pose, the map and the
//! barrier switch are saved.
use super::park_parts::{Part, placed};
use super::pieces::{PieceGeometry, PieceKind, SLAB_SIZE, Surface, prop_box};
use super::PlacedPieces;
use crate::physics::SkaterRuntime;
use crate::xr::rig::Rig;
use crate::xr::spawn::spawn_matrix;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, PI};

/// World scale a new park starts at: the base is about 1 m wide.
pub(crate) const START_SCALE: f32 = 32.0;
/// Wait between collision rebuild and putting the skater on, seconds.
const RESPAWN_DELAY: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Park {
    /// Tracking-space (STAGE) centre of the base's bottom.
    pub position: [f32; 3],
    /// Turn about +Y; the park's -z faces away from where it was placed from.
    pub yaw: f32,
    pub fence: bool,
    /// Index into [`MAPS`].
    #[serde(default)]
    pub map: usize,
}

/// Scenery on the deck. All of it collides; planter rims and bench seats
/// are grindable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prop {
    /// A planter with a tree, facing doesn't matter.
    Planter,
    /// A bench, facing +z at turn 0.
    Bench,
    Lamp,
    Flag,
}

/// (part, base point on the deck (x, height, z), turn).
type PartSpec = (Part, [f32; 3], f32);
type PropSpec = (Prop, [f32; 2], f32);

pub(crate) struct ParkMap {
    pub name: &'static str,
    parts: &'static [PartSpec],
    props: &'static [PropSpec],
    /// Start, deck-relative (x, z); the skater faces -z.
    spawn: [f32; 2],
    /// Coins (x, height above the deck, z); none means one over every part.
    coins: Option<&'static [[f32; 3]]>,
}

const CORNERS: [PropSpec; 4] = [
    (Prop::Planter, [-14.4, -10.4], 0.0),
    (Prop::Planter, [14.4, -10.4], 0.0),
    (Prop::Planter, [-14.4, 10.4], 0.0),
    (Prop::Planter, [14.4, 10.4], 0.0),
];
const LAMPS: [PropSpec; 4] = [
    (Prop::Lamp, [-15.4, -6.0], 0.0),
    (Prop::Lamp, [15.4, -6.0], 0.0),
    (Prop::Lamp, [-15.4, 6.0], 0.0),
    (Prop::Lamp, [15.4, 6.0], 0.0),
];
const DEG: f32 = PI / 180.0;

const fn quarter(width: f32, height: f32, run: f32, deck: f32) -> Part {
    Part::Quarter { width, height, run, deck }
}
const fn bank(width: f32, height: f32, run: f32, deck: f32) -> Part {
    Part::Bank { width, height, run, deck }
}
const fn ledge(length: f32, height: f32, width: f32) -> Part {
    Part::Ledge { length, height, width }
}
const fn flat_rail(length: f32, height: f32) -> Part {
    Part::FlatRail { length, height }
}
const fn statue(size: f32) -> Part {
    Part::Piece(PieceKind::Statue, size)
}

/// Deck 32 x 24 game metres, x across, -z away from the player. Turns: a
/// quarter pipe or bank opens towards +z at 0, +x at +90 degrees, -x at -90
/// and -z at 180, as in the redesign.
pub(crate) const MAPS: [ParkMap; 5] = [
    // 1. Flow & Funbox: quarter pipes with decks down both sides, a back
    // quarter under the board, a full funbox (hubba and rail) north of the
    // statue and a kicker-to-kicker gap south of it.
    ParkMap {
        name: "Flow & Funbox",
        parts: &[
            (quarter(11.0, 1.6, 2.2, 1.6), [-11.5, 0.0, 0.0], 90.0 * DEG),
            (quarter(11.0, 1.6, 2.2, 1.6), [11.5, 0.0, 0.0], -90.0 * DEG),
            (quarter(9.0, 1.45, 1.8, 1.2), [0.0, 0.0, -6.9], 0.0),
            (
                Part::Funbox { length: 8.2, width: 2.8, height: 0.75, top: 3.0, hubba: true, rail: true, gap: false },
                [0.0, 0.0, -2.8],
                0.0,
            ),
            (
                Part::Funbox { length: 8.6, width: 2.5, height: 0.8, top: 2.2, hubba: false, rail: true, gap: true },
                [0.0, 0.0, 4.2],
                0.0,
            ),
            (statue(0.6), [0.0, 0.0, 0.6], 0.0),
        ],
        props: &[
            CORNERS[0], CORNERS[1], CORNERS[2], CORNERS[3], LAMPS[0], LAMPS[1], LAMPS[2], LAMPS[3],
            (Prop::Bench, [-4.2, 7.6], 0.0),
            (Prop::Bench, [4.2, 7.6], 0.0),
            (Prop::Flag, [-6.0, -11.5], 0.0),
            (Prop::Flag, [6.0, -11.5], 0.0),
        ],
        spawn: [0.0, 7.4],
        coins: Some(&[[-9.8, 2.1, -2.8], [0.0, 1.6, -2.8], [9.8, 2.1, -2.8], [0.0, 2.1, -6.2], [-9.8, 2.1, 4.2], [0.0, 1.6, 4.2], [9.8, 2.1, 4.2]]),
    },
    // 2. Two-level plaza: a raised concrete deck on the west with stairs, a
    // bank and a quarter coming down off it; a quarter and bank wall on the
    // east; plaza ledges, a manual pad and a flat bar in between. The statue
    // stands on the upper deck.
    ParkMap {
        name: "Plaza Dois Níveis",
        parts: &[
            (Part::DeckBox { width: 7.6, height: 1.1, depth: 11.6 }, [-9.4, 0.0, -1.2], 0.0),
            (Part::Stairs { width: 3.2, height: 1.1, run: 2.8, steps: 6 }, [-4.2, 0.0, -4.5], 90.0 * DEG),
            (bank(3.8, 1.1, 2.8, 0.05), [-4.2, 0.0, 0.0], 90.0 * DEG),
            (quarter(2.8, 1.1, 2.2, 0.6), [-4.2, 0.0, 3.4], 90.0 * DEG),
            (quarter(6.4, 1.55, 2.1, 1.5), [11.2, 0.0, -3.2], -90.0 * DEG),
            (bank(5.6, 1.35, 2.2, 1.4), [11.2, 0.0, 3.2], -90.0 * DEG),
            (ledge(6.2, 0.5, 1.1), [2.2, 0.0, -6.2], 0.0),
            (ledge(5.8, 0.45, 1.6), [2.5, 0.0, 3.6], 0.0),
            (flat_rail(4.4, 0.52), [3.2, 0.0, -0.5], 0.0),
            (statue(0.6), [-10.5, 1.1, 2.0], 0.0),
        ],
        props: &[
            CORNERS[0], CORNERS[1], CORNERS[2], CORNERS[3], LAMPS[0], LAMPS[1], LAMPS[2], LAMPS[3],
            (Prop::Bench, [-4.5, 10.8], PI),
            (Prop::Bench, [4.5, 10.8], PI),
            (Prop::Flag, [-6.0, -11.5], 0.0),
            (Prop::Flag, [6.0, -11.5], 0.0),
        ],
        spawn: [7.0, 6.8],
        coins: Some(&[[-9.0, 1.7, -4.5], [-4.2, 1.3, -4.5], [2.0, 1.0, -5.8], [9.8, 2.0, -3.5], [9.8, 1.8, 3.0], [2.5, 1.1, 3.8], [-4.2, 1.1, 0.0]]),
    },
    // 3. Bowl and halfpipe arena: tall quarters with decks north, west and
    // east, two front quarters with a gate between them, and a four-way hip
    // pyramid (with a rail over it) in the middle. The statue stands on the
    // north deck.
    ParkMap {
        name: "Arena 360",
        parts: &[
            (quarter(17.0, 1.75, 2.2, 1.4), [0.0, 0.0, -6.8], 0.0),
            (quarter(10.8, 1.75, 2.2, 1.6), [-11.5, 0.0, 0.2], 90.0 * DEG),
            (quarter(10.8, 1.75, 2.2, 1.6), [11.5, 0.0, 0.2], -90.0 * DEG),
            (quarter(7.2, 1.55, 1.9, 1.3), [-5.8, 0.0, 6.8], 180.0 * DEG),
            (quarter(7.2, 1.55, 1.9, 1.3), [5.8, 0.0, 6.8], 180.0 * DEG),
            (Part::Pyramid { base_w: 7.6, base_d: 5.6, top_w: 3.6, top_d: 2.6, height: 0.72 }, [0.0, 0.0, 0.6], 0.0),
            (flat_rail(3.8, 0.5), [0.0, 0.72, 0.6], 0.0),
            (statue(0.5), [-6.0, 1.75, -7.9], 0.0),
        ],
        props: &[
            CORNERS[0], CORNERS[1], CORNERS[2], CORNERS[3], LAMPS[0], LAMPS[1], LAMPS[2], LAMPS[3],
            (Prop::Bench, [-4.5, 10.8], PI),
            (Prop::Bench, [4.5, 10.8], PI),
            (Prop::Flag, [-6.0, -11.5], 0.0),
            (Prop::Flag, [6.0, -11.5], 0.0),
        ],
        spawn: [0.0, 6.6],
        coins: Some(&[[-5.5, 2.2, -6.2], [0.0, 2.3, -6.2], [5.5, 2.2, -6.2], [-10.2, 2.2, 0.6], [0.0, 1.5, 0.6], [10.2, 2.2, 0.6], [-6.2, 2.0, 6.2], [6.2, 2.0, 6.2]]),
    },
    // 4. Pro course: a big section at the back (a high drop-in deck with a
    // central stair set and a bank either side), quarters with decks at both
    // ends, bevelled plaza ledges round the statue and a flat bar and manual
    // pad along the front.
    ParkMap {
        name: "Pro Course SLS",
        parts: &[
            (Part::DeckBox { width: 13.6, height: 1.45, depth: 2.2 }, [0.0, 0.0, -7.6], 0.0),
            (Part::Stairs { width: 3.6, height: 1.45, run: 2.8, steps: 7 }, [0.0, 0.0, -5.1], 0.0),
            (bank(5.0, 1.45, 2.8, 0.05), [-4.3, 0.0, -5.1], 0.0),
            (bank(5.0, 1.45, 2.8, 0.05), [4.3, 0.0, -5.1], 0.0),
            (quarter(9.2, 1.6, 2.2, 1.6), [-11.5, 0.0, 1.0], 90.0 * DEG),
            (quarter(9.2, 1.6, 2.2, 1.6), [11.5, 0.0, 1.0], -90.0 * DEG),
            (ledge(4.2, 0.48, 0.85), [0.0, 0.0, -1.8], 0.0),
            (ledge(4.2, 0.48, 0.85), [0.0, 0.0, 3.0], 0.0),
            (ledge(3.8, 0.48, 0.85), [-2.8, 0.0, 0.6], 90.0 * DEG),
            (ledge(3.8, 0.48, 0.85), [2.8, 0.0, 0.6], 90.0 * DEG),
            (flat_rail(4.2, 0.52), [5.5, 0.0, 5.4], 0.0),
            (ledge(3.8, 0.42, 1.5), [-5.5, 0.0, 5.4], 0.0),
            (statue(0.55), [0.0, 0.0, 0.6], 0.0),
        ],
        props: &[
            CORNERS[0], CORNERS[1], CORNERS[2], CORNERS[3], LAMPS[0], LAMPS[1], LAMPS[2], LAMPS[3],
            (Prop::Bench, [-4.5, 10.8], PI),
            (Prop::Bench, [4.5, 10.8], PI),
            (Prop::Flag, [-9.0, -11.5], 0.0),
            (Prop::Flag, [9.0, -11.5], 0.0),
        ],
        spawn: [0.0, 7.4],
        coins: Some(&[[0.0, 2.2, -5.5], [-4.2, 1.6, -5.5], [4.2, 1.6, -5.5], [0.0, 1.2, -2.1], [0.0, 1.2, 3.3], [-9.8, 2.1, 1.0], [9.8, 2.1, 1.0]]),
    },
    // 5. A closed bowl park: quarter pipes all round (a gap at the front to
    // ride in), the statue in the middle with a box, rail and ledge to hit
    // between walls.
    ParkMap {
        name: "Bowl Fechado",
        parts: &[
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [-10.0, 0.0, -10.4], 0.0),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [-6.0, 0.0, -10.4], 0.0),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [-2.0, 0.0, -10.4], 0.0),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [2.0, 0.0, -10.4], 0.0),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [6.0, 0.0, -10.4], 0.0),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [10.0, 0.0, -10.4], 0.0),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [-10.0, 0.0, 10.4], PI),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [-6.0, 0.0, 10.4], PI),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [6.0, 0.0, 10.4], PI),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [10.0, 0.0, 10.4], PI),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [-14.4, 0.0, -4.0], FRAC_PI_2),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [-14.4, 0.0, 0.0], FRAC_PI_2),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [-14.4, 0.0, 4.0], FRAC_PI_2),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [14.4, 0.0, -4.0], -FRAC_PI_2),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [14.4, 0.0, 0.0], -FRAC_PI_2),
            (Part::Piece(PieceKind::QuarterPipe, 1.0), [14.4, 0.0, 4.0], -FRAC_PI_2),
            (statue(0.8), [0.0, 0.0, 0.0], 0.0),
            (Part::Piece(PieceKind::FunBox, 0.8), [-6.0, 0.0, 0.0], FRAC_PI_2),
            (Part::Piece(PieceKind::Rail, 0.6), [6.0, 0.0, 0.0], FRAC_PI_2),
            (Part::Piece(PieceKind::Ledge, 0.6), [0.0, 0.0, -5.0], 0.0),
        ],
        props: &[
            CORNERS[0], CORNERS[1], CORNERS[2], CORNERS[3],
            (Prop::Bench, [-2.6, 11.3], PI),
            (Prop::Bench, [2.6, 11.3], PI),
            (Prop::Lamp, [-15.4, -7.5], 0.0),
            (Prop::Lamp, [15.4, -7.5], 0.0),
            (Prop::Lamp, [-15.4, 7.5], 0.0),
            (Prop::Lamp, [15.4, 7.5], 0.0),
            (Prop::Flag, [-13.8, -8.0], 0.0),
            (Prop::Flag, [13.8, -8.0], 0.0),
        ],
        spawn: [0.0, 6.5],
        coins: None,
    },
];

/// A park part in the park's own frame: the part, its base point (game
/// metres from the base's bottom centre) and turn.
pub(crate) type LocalPart = (Part, Vec3, f32);

fn map(index: usize) -> &'static ParkMap {
    &MAPS[index % MAPS.len()]
}

/// The base, the map's parts and (if on) the barrier, in the park's frame.
pub(crate) fn local_parts(map_index: usize, fence: bool) -> Vec<LocalPart> {
    let deck = Vec3::Y * SLAB_SIZE.y;
    let mut out = vec![(Part::Piece(PieceKind::Slab, 1.0), Vec3::ZERO, 0.0)];
    out.extend(map(map_index).parts.iter().map(|&(part, at, turn)| (part, deck + Vec3::from_array(at), turn)));
    if fence {
        let (hx, hz) = (SLAB_SIZE.x * 0.5 - 0.05, SLAB_SIZE.z * 0.5 - 0.05);
        // Fence size 1 is 16 m long.
        let fence = |length: f32| Part::Piece(PieceKind::Fence, length / 16.0);
        out.extend([
            (fence(SLAB_SIZE.x), deck - Vec3::Z * hz, 0.0),
            (fence(SLAB_SIZE.x), deck + Vec3::Z * hz, PI),
            (fence(SLAB_SIZE.z), deck - Vec3::X * hx, FRAC_PI_2),
            (fence(SLAB_SIZE.z), deck + Vec3::X * hx, -FRAC_PI_2),
        ]);
    }
    out
}

/// The map's props in the park's frame: kind, base point on the deck, turn.
pub(crate) fn local_props(map_index: usize) -> Vec<(Prop, Vec3, f32)> {
    map(map_index)
        .props
        .iter()
        .map(|&(prop, [x, z], turn)| (prop, Vec3::new(x, SLAB_SIZE.y, z), turn))
        .collect()
}

/// Collision for one prop, in the park's frame.
pub(crate) fn prop_collision(prop: Prop, at: Vec3, turn: f32) -> PieceGeometry {
    let (size, grind) = match prop {
        Prop::Planter => (Vec3::new(2.2, 0.7, 2.2), true),
        Prop::Bench => (Vec3::new(2.4, 0.56, 0.6), true),
        Prop::Lamp => (Vec3::new(0.2, 4.0, 0.2), false),
        Prop::Flag => (Vec3::new(0.16, 6.0, 0.16), false),
    };
    placed(prop_box(Vec2::ZERO, size, grind), at, turn)
}

/// Everything solid in the park, in its frame, with its surface: the base,
/// the parts, the barrier and the props.
pub(crate) fn park_geometry(map_index: usize, fence: bool) -> Vec<(Surface, PieceGeometry)> {
    let mut out: Vec<(Surface, PieceGeometry)> = local_parts(map_index, fence)
        .into_iter()
        .flat_map(|(part, at, turn)| part.shapes().into_iter().map(move |(s, g)| (s, placed(g, at, turn))))
        .collect();
    out.extend(local_props(map_index).into_iter().map(|(prop, at, turn)| (Surface::Granite, prop_collision(prop, at, turn))));
    out
}

/// Coins, deck-relative: the map's own, or one over the middle of every part.
pub(crate) fn coin_spots(map_index: usize) -> Vec<Vec3> {
    let m = map(map_index);
    if let Some(coins) = m.coins {
        return coins.iter().map(|&c| Vec3::from_array(c)).collect();
    }
    m.parts
        .iter()
        .filter(|(part, ..)| !matches!(part, Part::Piece(PieceKind::Statue, _)))
        .map(|&(part, at, turn)| {
            let (lo, hi) = part
                .shapes()
                .iter()
                .flat_map(|(_, g)| g.all_quads().flatten().copied().collect::<Vec<_>>())
                .fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(lo, hi), p| (lo.min(p), hi.max(p)));
            let middle = Quat::from_rotation_y(turn) * ((lo + hi) * 0.5).with_y(0.0);
            Vec3::from_array(at) + middle + Vec3::Y * (hi.y + 1.4)
        })
        .collect()
}

impl Park {
    fn frame(&self) -> (Vec3, Quat) {
        (Vec3::from_array(self.position), Quat::from_rotation_y(self.yaw))
    }

    pub(crate) fn map(&self) -> &'static ParkMap {
        map(self.map)
    }

    /// The park's frame in the game world (game metres, unscaled).
    pub(crate) fn world_transform(&self, rig: &Rig) -> Option<Transform> {
        let root = rig.root_transform()?;
        let (base, turn) = self.frame();
        Some(Transform {
            translation: root.transform_point(base),
            rotation: root.rotation * turn,
            scale: Vec3::ONE,
        })
    }

    /// World-space spawn point and riding direction.
    fn spawn(&self, rig: &Rig) -> Option<(Vec3, Vec3)> {
        let park = self.world_transform(rig)?;
        let [x, z] = self.map().spawn;
        let point = park.transform_point(Vec3::new(x, SLAB_SIZE.y, z));
        Some((point, park.rotation * Vec3::NEG_Z))
    }
}

impl PlacedPieces {
    /// Put the park down (replacing any other) and send the skater onto it.
    pub(crate) fn set_park(&mut self, park: Option<Park>) {
        self.park = park;
        self.revision += 1;
        self.respawn = park.map(|_| RESPAWN_DELAY);
    }
}

/// Put the skater on the park once it is placed, moved, resized or its map
/// changed. Falling off it is fine: the skater carries on in the room.
pub(super) fn keep_on_park(
    time: Res<Time<Real>>,
    rig: Res<Rig>,
    skater: Option<ResMut<SkaterRuntime>>,
    mut placed: ResMut<PlacedPieces>,
    menu: Res<crate::xr::menu::XrMenu>,
) {
    // Collision follows a carried park only once it is put down.
    if menu.moving_park {
        return;
    }
    let (Some(park), Some(mut skater)) = (placed.park, skater) else {
        return;
    };
    let Some(wait) = placed.respawn.as_mut() else { return };
    *wait -= time.delta_secs();
    if *wait > 0.0 || placed.busy() {
        return;
    }
    placed.respawn = None;
    let Some((point, forward)) = park.spawn(&rig) else { return };
    if let Err(e) = skater.travel_to(spawn_matrix(point, forward)) {
        warn!("PARK put-on: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Footprint (x, z) bounds of some geometry.
    fn footprint(g: &PieceGeometry) -> (Vec2, Vec2) {
        g.all_quads().flatten().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| {
            (lo.min(p.xz()), hi.max(p.xz()))
        })
    }

    fn inside_deck(p: Vec2, margin: f32) -> bool {
        p.x.abs() <= SLAB_SIZE.x * 0.5 - margin && p.y.abs() <= SLAB_SIZE.z * 0.5 - margin
    }

    #[test]
    fn every_map_fits_on_the_deck() {
        for (m, map) in MAPS.iter().enumerate() {
            for (part, at, turn) in local_parts(m, false).into_iter().skip(1) {
                for (_, g) in part.shapes() {
                    let (lo, hi) = footprint(&placed(g, at, turn));
                    assert!(inside_deck(lo, -0.01) && inside_deck(hi, -0.01), "{}: {part:?} leaves the deck", map.name);
                }
            }
            for (prop, at, turn) in local_props(m) {
                let (lo, hi) = footprint(&prop_collision(prop, at, turn));
                assert!(inside_deck(lo, -0.01) && inside_deck(hi, -0.01), "{}: {prop:?} leaves the deck", map.name);
            }
            let [x, z] = map.spawn;
            assert!(inside_deck(Vec2::new(x, z), 1.0), "{}: spawn on the deck", map.name);
        }
    }

    #[test]
    fn the_bowl_has_no_overlaps() {
        let m = MAPS.len() - 1;
        let boxes: Vec<(Vec2, Vec2)> = local_parts(m, false)
            .into_iter()
            .skip(1)
            .flat_map(|(part, at, turn)| part.shapes().into_iter().map(move |(_, g)| footprint(&placed(g, at, turn))))
            .collect();
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                let overlap = (a.1.min(b.1) - a.0.max(b.0)).min_element();
                assert!(overlap < 0.35, "overlap {overlap:.2}");
            }
        }
    }

    #[test]
    fn ramps_open_onto_the_deck() {
        for map in &MAPS {
            for &(part, at, turn) in map.parts {
                if part.opens() {
                    let open = (Quat::from_rotation_y(turn) * Vec3::Z).xz();
                    let at = Vec2::new(at[0], at[2]);
                    assert!(inside_deck(at + open * 4.0, 0.5), "{}: {part:?} at {at} opens off the deck", map.name);
                }
            }
        }
    }

    #[test]
    fn coins_float_over_the_deck() {
        for (m, map) in MAPS.iter().enumerate() {
            let coins = coin_spots(m);
            assert!(coins.len() >= 5, "{}", map.name);
            assert!(coins.iter().all(|c| inside_deck(c.xz(), 0.0) && c.y > 0.9), "{}", map.name);
        }
    }

    #[test]
    fn every_surface_is_closed_and_finite() {
        for m in 0..MAPS.len() {
            for (_, g) in park_geometry(m, true) {
                assert!(g.all_quads().flatten().all(|p| p.is_finite()));
            }
        }
    }

    /// `cargo test ... dump_park_maps -- --ignored` writes every map's faces
    /// (park frame, with their surface) to $PARK_DUMP for the preview sheet.
    #[test]
    #[ignore]
    fn dump_park_maps() {
        let path = std::env::var("PARK_DUMP").expect("set PARK_DUMP");
        let mut maps = Vec::new();
        for (m, map) in MAPS.iter().enumerate() {
            let mut faces = Vec::new();
            for (surface, g) in park_geometry(m, true) {
                for q in g.all_quads() {
                    faces.push(serde_json::json!({"s": format!("{surface:?}"), "q": q.map(|p| p.to_array())}));
                }
            }
            let statue: Vec<_> = map
                .parts
                .iter()
                .filter(|(p, ..)| matches!(p, Part::Piece(PieceKind::Statue, _)))
                .map(|&(_, at, _)| (Vec3::from_array(at) + Vec3::Y * SLAB_SIZE.y).to_array())
                .collect();
            let coins: Vec<_> = coin_spots(m).iter().map(|c| (*c + Vec3::Y * SLAB_SIZE.y).to_array()).collect();
            maps.push(serde_json::json!({"name": map.name, "faces": faces, "coins": coins, "spawn": map.spawn, "statue": statue}));
        }
        std::fs::write(path, serde_json::to_vec(&maps).unwrap()).unwrap();
    }
}
