//! Trick scoring you can see in mixed reality, and MarcoVR coins to chase.
//!
//! The game already scores every trick (`scoring_runtime`, the retail
//! ScoreModule port); its own trick display is a flat HUD the headset never
//! shows. This board floats over the back of the Marco park (or above the
//! skater when there is no park) with the current trick, the line's points
//! and multiplier, and the session total and best line. Text is built from
//! the glyph atlas in `marcovr`. Without a park the board stays where it was
//! put (the menu's "Placar na minha frente"), not on the skater.
//!
//! Under the board stands the park's big screen (`tv`).
//!
//! VIP coins (the VR Tutoriais Marco Pereira group medal) float over every
//! obstacle of the park's map; skating through one chimes, adds a bonus and
//! it comes back a little later.
use super::build::PlacedPieces;
use super::build::park::coin_spots;
use super::build::pieces::SLAB_SIZE;
use super::marcovr::{MarcoTextures, head_pose};
use super::rig::Rig;
use crate::physics::{GamePhysics, SkaterRuntime};
use crate::skate_audio::StreamPcm;
use bevy::prelude::*;
use bevy_mod_xr::camera::XrCamera;
use skate_core::physics::board::BodyId;

/// Board size, game metres (texture 1024 x 448).
const BOARD_W: f32 = 13.0;
const BOARD_H: f32 = BOARD_W * 448.0 / 1024.0;
/// Over the back of the park, deck-relative.
const BOARD_AT: Vec3 = Vec3::new(0.0, 13.6, -12.6);
/// The board's size over a park, under the full size.
const PARK_BOARD_SCALE: f32 = 0.8;

/// With no park: real width of the board, and how far in front of the eyes
/// it is put, metres.
const FIXED_WIDTH_M: f32 = 0.5;
const FIXED_AHEAD_M: f32 = 1.4;
/// How long a finished line stays up, seconds.
const RESULT_SECONDS: f32 = 3.0;

const COIN_POINTS: i64 = 250;
const COIN_RADIUS: f32 = 1.2;
const COIN_SIZE: f32 = 0.8;
const COIN_RETURN_SECONDS: f32 = 25.0;
/// Coin entities kept ready; a map uses as many as it has obstacles.
const COIN_POOL: usize = 24;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Line {
    Trick,
    Points,
    Total,
}

#[derive(Component)]
struct Board;

#[derive(Component)]
struct Coin(usize);

#[derive(Resource, Default)]
pub(crate) struct Score {
    total: i64,
    best: i64,
    /// The open line's best points and multiplier so far.
    peak: (i32, f32),
    was_open: bool,
    /// A finished line or coin message, and how long it stays.
    result: Option<(String, String, f32)>,
    /// Per coin of the current map: seconds until it is back.
    coin_away: Vec<f32>,
    coin_map: Option<usize>,
    texts: [String; 3],
    /// Where the board stands without a park (tracking space).
    board_pose: Option<Transform>,
    /// The player put it there: it stays there even over a park.
    board_manual: bool,
    /// The menu asked for the board in front of the player.
    pub(crate) place_board_here: bool,
}

impl Score {
    /// The session's total points.
    pub(crate) fn total(&self) -> i64 {
        self.total
    }
    /// The best single line's points.
    pub(crate) fn best(&self) -> i64 {
        self.best
    }
}

#[derive(Resource)]
struct Chime(Handle<StreamPcm>);

/// A bright two-note "plim", synthesised once.
fn chime() -> StreamPcm {
    const RATE: u32 = 48_000;
    let len = (RATE as f32 * 0.45) as usize;
    let samples = (0..len)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let note = |f: f32, start: f32| {
                let t = t - start;
                if t < 0.0 {
                    return 0.0;
                }
                let env = (t * 400.0).min(1.0) * (-t * 9.0).exp();
                env * ((std::f32::consts::TAU * f * t).sin() + 0.3 * (std::f32::consts::TAU * 2.0 * f * t).sin())
            };
            let v = 0.35 * (note(1318.5, 0.0) + note(1975.5, 0.08));
            (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
        })
        .collect();
    StreamPcm::mono(samples, RATE)
}

pub(crate) struct ScorePlugin;

impl Plugin for ScorePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Score>()
            .add_systems(Startup, spawn.after(super::marcovr::load_textures))
            .add_systems(Update, (keep_score, collect_coins, place, write_lines).chain());
    }
}

fn unlit(texture: Option<Handle<Image>>, color: Color, alpha: AlphaMode) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        base_color_texture: texture,
        alpha_mode: alpha,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

fn spawn(
    mut commands: Commands,
    textures: Option<Res<MarcoTextures>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sounds: ResMut<Assets<StreamPcm>>,
) {
    commands.insert_resource(Chime(sounds.add(chime())));
    let Some(textures) = textures else { return };
    let board = commands
        .spawn((
            Board,
            Mesh3d(meshes.add(Rectangle::new(BOARD_W, BOARD_H))),
            MeshMaterial3d(materials.add(unlit(Some(textures.score_board.clone()), Color::WHITE, AlphaMode::Blend))),
            Transform::default(),
            Visibility::Hidden,
        ))
        .id();
    for (line, y, color) in [
        (Line::Trick, 0.35, Color::srgb(1.0, 0.83, 0.0)),
        (Line::Points, -0.95, Color::WHITE),
        (Line::Total, -2.0, Color::srgb(0.494, 0.784, 0.949)),
    ] {
        commands.spawn((
            ChildOf(board),
            line,
            Mesh3d(meshes.add(Rectangle::new(0.01, 0.01))),
            MeshMaterial3d(materials.add(unlit(Some(textures.font.clone()), color, AlphaMode::Mask(0.4)))),
            Transform::from_xyz(0.0, y, 0.05),
        ));
    }
    let coin_mesh = meshes.add(Rectangle::new(1.0, 1.0));
    let coin_material = materials.add(unlit(Some(textures.vip_coin.clone()), Color::WHITE, AlphaMode::Mask(0.5)));
    for i in 0..COIN_POOL {
        commands.spawn((
            Coin(i),
            Mesh3d(coin_mesh.clone()),
            MeshMaterial3d(coin_material.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

/// Readable trick names from the scorer's composed name, e.g.
/// `#ID_TRICK_AIR_FS_SPIN 360` -> `FS 360`.
fn trick_name(raw: &str) -> String {
    raw.trim_start_matches('#')
        .split_whitespace()
        .map(|token| match token {
            "ID_TRICK_AIR" => "Air".to_owned(),
            "ID_TRICK_AIR_FS_SPIN" => "FS".to_owned(),
            "ID_TRICK_AIR_BS_SPIN" => "BS".to_owned(),
            "ID_TRICK_AIR_METRICS_FRONTFLIP" => "Frontflip".to_owned(),
            "ID_TRICK_AIR_METRICS_BACKFLIP" => "Backflip".to_owned(),
            t => t
                .trim_start_matches("ID_TRICK_")
                .split('_')
                .filter(|w| !w.is_empty())
                .map(|w| {
                    let w = w.to_lowercase();
                    let mut c = w.chars();
                    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
                })
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn keep_score(time: Res<Time<Real>>, skater: Option<Res<SkaterRuntime>>, mut score: ResMut<Score>) {
    let Some(skater) = skater else { return };
    let scoring = &skater.scoring;
    let hud = scoring.hud_input();
    let open = scoring.sequence_open();
    if let Some((_, _, t)) = score.result.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            score.result = None;
        }
    }
    if open {
        if !score.was_open {
            score.peak = (0, 1.0);
        }
        let (points, multiplier) = score.peak;
        score.peak = (points.max(hud.sequence_score), multiplier.max(hud.multiplier));
    } else if score.was_open {
        let (points, multiplier) = score.peak;
        let gained = (points as f32 * multiplier.max(1.0)).round() as i64;
        info!(
            "SCORE line closed: {} points x{multiplier:.1} = {gained} (line {}, '{}')",
            points, hud.line_score, hud.trick_name
        );
        if gained > 0 {
            score.total += gained;
            score.best = score.best.max(gained);
            let label = if hud.sketchy { "QUASE!" } else { "MANDOU BEM!" };
            score.result = Some((label.to_owned(), format!("+{gained}"), RESULT_SECONDS));
        }
    }
    score.was_open = open;
    let lines = if open {
        let (points, multiplier) = score.peak;
        [trick_name(&hud.trick_name), format!("{points}  x{multiplier:.1}")]
    } else if let Some((label, points, _)) = &score.result {
        [label.clone(), points.clone()]
    } else {
        ["Manda uma manobra!".to_owned(), String::new()]
    };
    let total = format!("TOTAL {}   RECORDE {}", score.total, score.best);
    score.texts = [lines[0].clone(), lines[1].clone(), total];
}

fn deck_point(physics: &GamePhysics) -> Vec3 {
    let t = physics.board.part_transforms()[BodyId::Deck.index()].translation;
    Vec3::new(t.x, t.y, t.z)
}

#[allow(clippy::too_many_arguments)]
fn collect_coins(
    mut commands: Commands,
    time: Res<Time<Real>>,
    chime: Option<Res<Chime>>,
    rig: Res<Rig>,
    placed: Res<PlacedPieces>,
    physics: Option<Res<GamePhysics>>,
    mut score: ResMut<Score>,
    mut coins: Query<(&Coin, &mut Transform, &mut Visibility)>,
) {
    let map = placed.park.map(|p| p.map);
    let spots = map.map(coin_spots).unwrap_or_default();
    if score.coin_map != map {
        score.coin_map = map;
        score.coin_away = vec![0.0; spots.len()];
    }
    let park = placed.park.and_then(|p| p.world_transform(&rig));
    let deck = physics.as_deref().map(deck_point);
    let spin = Quat::from_rotation_y(time.elapsed_secs() * 2.0);
    for (coin, mut transform, mut visibility) in &mut coins {
        let (Some(spot), Some(away)) = (spots.get(coin.0), score.coin_away.get_mut(coin.0)) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        *away = (*away - time.delta_secs()).max(0.0);
        let Some(park) = park.filter(|_| *away == 0.0) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let bob = (time.elapsed_secs() * 2.5 + coin.0 as f32).sin() * 0.15;
        let at = park.transform_point(*spot + Vec3::Y * (SLAB_SIZE.y + bob));
        *transform = Transform::from_translation(at).with_rotation(park.rotation * spin).with_scale(Vec3::splat(COIN_SIZE));
        *visibility = Visibility::Visible;
        if deck.is_some_and(|d| d.distance(at) < COIN_RADIUS) {
            *away = COIN_RETURN_SECONDS;
            score.total += COIN_POINTS;
            score.result = Some(("MOEDA VIP!".to_owned(), format!("+{COIN_POINTS}"), 1.5));
            if let Some(chime) = &chime {
                commands.spawn((
                    AudioPlayer(chime.0.clone()),
                    PlaybackSettings::DESPAWN.with_spatial(true),
                    Transform::from_translation(at),
                ));
            }
            info!("SCORE coin {} (+{COIN_POINTS})", coin.0);
        }
    }
}

/// Over the park's back edge, or (without a park, or once the player has
/// put it somewhere) standing still where it was put, facing the player.
#[allow(clippy::type_complexity)]
fn place(
    rig: Res<Rig>,
    placed: Res<PlacedPieces>,
    mut score: ResMut<Score>,
    eyes: Query<&Transform, (With<XrCamera>, Without<Board>)>,
    mut board: Query<(&mut Transform, &mut Visibility), With<Board>>,
) {
    let Ok((mut transform, mut visibility)) = board.single_mut() else { return };
    let head = head_pose(eyes.iter());
    let wants_fixed = score.place_board_here || (placed.park.is_none() && score.board_pose.is_none());
    if let (true, Some(head)) = (wants_fixed, head) {
        let forward = (head.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
        let at = head.translation + forward * FIXED_AHEAD_M + Vec3::Y * 0.15;
        score.board_pose = Some(
            Transform::from_translation(at)
                .looking_to(forward, Vec3::Y)
                .with_scale(Vec3::splat(FIXED_WIDTH_M / BOARD_W)),
        );
        score.board_manual |= score.place_board_here;
        score.place_board_here = false;
    }
    let target = match (placed.park.and_then(|p| p.world_transform(&rig)), score.board_pose, rig.root_transform()) {
        (Some(park), _, _) if !score.board_manual => {
            Some(park.mul_transform(
                Transform::from_translation(BOARD_AT + Vec3::Y * SLAB_SIZE.y).with_scale(Vec3::splat(PARK_BOARD_SCALE)),
            ))
        }
        (_, Some(pose), Some(root)) => Some(root.mul_transform(pose)),
        _ => None,
    };
    match target {
        Some(t) => {
            if *transform != t {
                *transform = t;
            }
            *visibility = Visibility::Visible;
        }
        None => *visibility = Visibility::Hidden,
    }
}

fn write_lines(
    score: Res<Score>,
    textures: Option<Res<MarcoTextures>>,
    mut shown: Local<[String; 3]>,
    mut meshes: ResMut<Assets<Mesh>>,
    lines: Query<(&Line, &Mesh3d)>,
) {
    let Some(textures) = textures else { return };
    for (line, mesh) in &lines {
        let (i, height) = match line {
            Line::Trick => (0, 1.25),
            Line::Points => (1, 1.4),
            Line::Total => (2, 0.7),
        };
        if shown[i] == score.texts[i] {
            continue;
        }
        shown[i] = score.texts[i].clone();
        if let Some(target) = meshes.get_mut(&mesh.0) {
            *target = textures.font_metrics.text_mesh(&shown[i], height);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::trick_name;

    #[test]
    fn trick_names_read_like_the_game() {
        assert_eq!(trick_name("#ID_TRICK_AIR_FS_SPIN 360"), "FS 360");
        assert_eq!(trick_name("#ID_TRICK_KICKFLIP"), "Kickflip");
        assert_eq!(trick_name("#ID_TRICK_AIR ID_TRICK_AIR_METRICS_BACKFLIP"), "Air Backflip");
        assert_eq!(trick_name("#Varial_Heelflip"), "Varial Heelflip");
    }
}

