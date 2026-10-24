//! The Marco park's screens. Behind the park stands the big screen, playing
//! clips from the VR Tutoriais Marco Pereira channel (their sound, if packed,
//! turned right down and coming from the screen); on the right-hand side a
//! tall 9:16 screen plays a reels clip, silent; around them, LED panels cycle
//! through the channel's video thumbnails. Only shown with a park. Now and
//! then the big screen shows the scores instead: this player's total and
//! best line, or, in an online room, everyone's ranking.
//!
//! The clips are packed by `tools/make_tv_video.py`: small JPEG frames
//! (decoded one at a time as they come up) and mono 16-bit audio, looping
//! together. With nothing packed the screen says the videos are coming.
use super::build::PlacedPieces;
use super::build::pieces::SLAB_SIZE;
use super::marcovr::MarcoTextures;
use super::rig::Rig;
use crate::skate_audio::StreamPcm;
use bevy::asset::RenderAssetUsages;
use bevy::audio::Volume;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// The big screen: its 16:9 picture's width, and its centre over the back
/// edge, deck-relative, game metres.
const TV_W: f32 = 12.0;
const TV_AT: Vec3 = Vec3::new(0.0, 5.6, -12.7);
/// The clips' sound: very quiet.
const TV_VOLUME: f32 = 0.12;
const AUDIO_RATE: u32 = 22_050;
/// Thumbnail panels: picture width, and (deck-relative centre, turn).
const PANEL_W: f32 = 6.0;
const PANELS: [([f32; 3], f32); 4] = [
    ([-12.0, 4.4, -12.9], 25.0),
    ([12.0, 4.4, -12.9], -25.0),
    ([-16.9, 3.8, -2.0], 90.0),
    ([16.9, 3.8, -2.0], -90.0),
];
const PANEL_SLIDE_SECONDS: f32 = 6.0;
/// The vertical (reels) screen: picture width, deck-relative centre and turn;
/// on the right-hand side towards the front, facing across the park.
const REELS_W: f32 = 4.5;
const REELS_AT: Vec3 = Vec3::new(16.9, 4.8, 7.0);
const REELS_TURN: f32 = -90.0;

/// The big screen's cycle: the video this long, then the scores this long.
const VIDEO_SECONDS: f32 = 20.0;
const SCORES_SECONDS: f32 = 8.0;
/// Score lines on the big screen: the title and up to this many skaters.
const SCORE_ROWS: usize = 6;
const YELLOW: Color = Color::srgb(1.0, 0.83, 0.0);

static FRAMES: &[u8] = include_bytes!("marcovr/video/frames.bin");
static AUDIO: &[u8] = include_bytes!("marcovr/video/audio.pcm");
static REELS: &[u8] = include_bytes!("marcovr/video/reels_frames.bin");

/// The packed clip: frame rate and each frame's JPEG bytes.
struct Clip {
    fps: f32,
    frames: Vec<&'static [u8]>,
}

impl Clip {
    fn parse(bytes: &'static [u8]) -> Option<Self> {
        let word = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
        let (fps, count) = (word(0)?, word(4)?);
        let data = 8 + 8 * count;
        let frames = (0..count)
            .map(|i| {
                let (offset, len) = (word(8 + 8 * i)?, word(12 + 8 * i)?);
                bytes.get(data + offset..data + offset + len)
            })
            .collect::<Option<Vec<_>>>()?;
        (fps > 0 && !frames.is_empty()).then_some(Self { fps: fps as f32, frames })
    }

    fn seconds(&self) -> f32 {
        self.frames.len() as f32 / self.fps
    }
}

#[derive(Component)]
struct Tv;
#[derive(Component)]
struct Reels;
/// The scores shown over the big screen's picture, and their lines.
#[derive(Component)]
struct ScoreOverlay;
#[derive(Component)]
struct ScoreLine(usize);
#[derive(Component)]
struct TvSound;
#[derive(Component)]
struct Panel(usize);

/// A picture that plays a clip: its frames, and the images its material is
/// pointed at in turn (pointing the material at a different image is what
/// makes the renderer show the new frame).
#[derive(Component)]
struct Screen {
    clip: Clip,
    images: [Handle<Image>; 3],
    next_image: usize,
    shown: Option<usize>,
}

/// The big screen's sound (none when packed silent), and when the screens
/// started playing (with the park).
#[derive(Resource)]
struct Player {
    sound: Option<Handle<StreamPcm>>,
    started: Option<f32>,
}

pub(crate) struct TvPlugin;

impl Plugin for TvPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn.after(super::marcovr::load_textures))
            .add_systems(Update, (place, play, cycle_panels, scoreboard).chain());
    }
}

fn blank() -> Image {
    Image::new_fill(
        Extent3d { width: 4, height: 4, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn screen(texture: Option<Handle<Image>>) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: texture,
        unlit: true,
        ..default()
    }
}

/// A dark bezel with a glowing Marco-blue edge round a `w` x `h` picture.
fn bezel(commands: &mut Commands, parent: Entity, w: f32, h: f32, y: f32, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    commands.spawn((
        ChildOf(parent),
        Mesh3d(meshes.add(Cuboid::new(w + 0.9, h + 0.9, 0.2))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.494, 0.784, 0.949),
            emissive: LinearRgba::rgb(0.6, 1.2, 1.6),
            ..default()
        })),
        Transform::from_xyz(0.0, y, -0.25),
    ));
    commands.spawn((
        ChildOf(parent),
        Mesh3d(meshes.add(Cuboid::new(w + 0.6, h + 0.6, 0.4))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.06, 0.08),
            perceptual_roughness: 0.35,
            metallic: 0.4,
            ..default()
        })),
        Transform::from_xyz(0.0, y, -0.15),
    ));
}

fn spawn(
    mut commands: Commands,
    textures: Option<Res<MarcoTextures>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut sounds: ResMut<Assets<StreamPcm>>,
) {
    let samples: Vec<i16> = AUDIO.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    let player = Player {
        sound: (!samples.is_empty()).then(|| sounds.add(StreamPcm::mono(samples, AUDIO_RATE))),
        started: None,
    };
    let mut screen_of = |clip: Option<Clip>| {
        clip.map(|clip| Screen {
            clip,
            images: std::array::from_fn(|_| images.add(blank())),
            next_image: 0,
            shown: None,
        })
    };
    let tv_screen = screen_of(Clip::parse(FRAMES));
    let reels_screen = screen_of(Clip::parse(REELS));
    let Some(textures) = textures else {
        commands.insert_resource(player);
        return;
    };
    // The big screen and the channel strip on top of it.
    let tv_h = TV_W * 9.0 / 16.0;
    let strip_h = TV_W * 150.0 / 1024.0;
    let tv = commands.spawn((Tv, Transform::default(), Visibility::Hidden)).id();
    bezel(&mut commands, tv, TV_W, tv_h + strip_h, strip_h * 0.5, &mut meshes, &mut materials);
    let picture = commands
        .spawn((
            ChildOf(tv),
            Mesh3d(meshes.add(Rectangle::new(TV_W, tv_h))),
            MeshMaterial3d(materials.add(screen(Some(match &tv_screen {
                Some(s) => s.images[0].clone(),
                None => textures.tv_soon.clone(),
            })))),
            Transform::from_xyz(0.0, 0.0, 0.06),
        ))
        .id();
    if let Some(s) = tv_screen {
        commands.entity(picture).insert(s);
    }
    // The scores, over the picture (hidden until their turn).
    let overlay = commands.spawn((ChildOf(tv), ScoreOverlay, Transform::default(), Visibility::Hidden)).id();
    commands.spawn((
        ChildOf(overlay),
        Mesh3d(meshes.add(Rectangle::new(TV_W, tv_h))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.047, 0.055, 0.078),
            unlit: true,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.08),
    ));
    for i in 0..=SCORE_ROWS {
        let y = tv_h * 0.5 - 0.8 - i as f32 * 0.85 - if i > 0 { 0.3 } else { 0.0 };
        commands.spawn((
            ChildOf(overlay),
            ScoreLine(i),
            Mesh3d(meshes.add(Rectangle::new(0.01, 0.01))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: if i == 0 { YELLOW } else { Color::WHITE },
                base_color_texture: Some(textures.font.clone()),
                alpha_mode: AlphaMode::Mask(0.4),
                unlit: true,
                ..default()
            })),
            Transform::from_xyz(0.0, y, 0.1),
        ));
    }
    // The vertical screen, only when a reels clip is packed.
    if let Some(s) = reels_screen {
        let reels_h = REELS_W * 16.0 / 9.0;
        let reels = commands.spawn((Reels, Transform::default(), Visibility::Hidden)).id();
        bezel(&mut commands, reels, REELS_W, reels_h, 0.0, &mut meshes, &mut materials);
        commands.spawn((
            ChildOf(reels),
            Mesh3d(meshes.add(Rectangle::new(REELS_W, reels_h))),
            MeshMaterial3d(materials.add(screen(Some(s.images[0].clone())))),
            Transform::from_xyz(0.0, 0.0, 0.06),
            s,
        ));
    }
    commands.spawn((
        ChildOf(tv),
        Mesh3d(meshes.add(Rectangle::new(TV_W, strip_h))),
        MeshMaterial3d(materials.add(screen(Some(textures.tv_header.clone())))),
        Transform::from_xyz(0.0, (tv_h + strip_h) * 0.5, 0.06),
    ));
    // Thumbnail panels, each starting on a different video.
    let panel_h = PANEL_W * 9.0 / 16.0;
    let panel_mesh = meshes.add(Rectangle::new(PANEL_W, panel_h));
    for i in 0..PANELS.len() {
        let panel = commands.spawn((Transform::default(), Visibility::Hidden, Panel(i))).id();
        bezel(&mut commands, panel, PANEL_W, panel_h, 0.0, &mut meshes, &mut materials);
        let slide = textures.tv_slides[(i * 4) % textures.tv_slides.len()].clone();
        commands.spawn((
            ChildOf(panel),
            Mesh3d(panel_mesh.clone()),
            MeshMaterial3d(materials.add(screen(Some(slide)))),
            Transform::from_xyz(0.0, 0.0, 0.06),
        ));
    }
    commands.insert_resource(player);
}

/// The screens stand over the park (and only with one); the sound starts
/// with the park and stops without it.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place(
    mut commands: Commands,
    time: Res<Time<Real>>,
    rig: Res<Rig>,
    placed: Res<PlacedPieces>,
    player: Option<ResMut<Player>>,
    mut tv: Query<(Entity, &mut Transform, &mut Visibility), (With<Tv>, Without<Panel>, Without<Reels>)>,
    mut panels: Query<(&Panel, &mut Transform, &mut Visibility), (Without<Tv>, Without<Reels>)>,
    mut reels: Query<(&mut Transform, &mut Visibility), (With<Reels>, Without<Tv>, Without<Panel>)>,
    sound: Query<Entity, With<TvSound>>,
) {
    let (Some(mut player), Ok((tv, mut transform, mut visibility))) = (player, tv.single_mut()) else { return };
    let park = placed.park.and_then(|p| p.world_transform(&rig));
    let deck = |at: [f32; 3]| Vec3::from_array(at) + Vec3::Y * SLAB_SIZE.y;
    for (panel, mut t, mut v) in &mut panels {
        let (at, turn) = PANELS[panel.0];
        match park {
            Some(park) => {
                *t = park.mul_transform(Transform::from_translation(deck(at)).with_rotation(Quat::from_rotation_y(turn.to_radians())));
                *v = Visibility::Visible;
            }
            None => *v = Visibility::Hidden,
        }
    }
    for (mut t, mut v) in &mut reels {
        match park {
            Some(park) => {
                let local = Transform::from_translation(deck(REELS_AT.to_array()))
                    .with_rotation(Quat::from_rotation_y(REELS_TURN.to_radians()));
                *t = park.mul_transform(local);
                *v = Visibility::Visible;
            }
            None => *v = Visibility::Hidden,
        }
    }
    let Some(park) = park else {
        *visibility = Visibility::Hidden;
        for e in &sound {
            commands.entity(e).despawn();
        }
        player.started = None;
        return;
    };
    let target = park.mul_transform(Transform::from_translation(deck(TV_AT.to_array())));
    if *transform != target {
        *transform = target;
    }
    *visibility = Visibility::Visible;
    if player.started.is_none() {
        player.started = Some(time.elapsed_secs());
        if let Some(sound) = player.sound.clone() {
            commands.spawn((
                ChildOf(tv),
                TvSound,
                AudioPlayer(sound),
                PlaybackSettings::LOOP.with_spatial(true).with_volume(Volume::Linear(TV_VOLUME)),
                Transform::default(),
            ));
        }
    }
}

/// Each screen shows the frame that matches the time since the park went up.
fn play(
    time: Res<Time<Real>>,
    player: Option<Res<Player>>,
    mut screens: Query<(&mut Screen, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(started) = player.and_then(|p| p.started) else { return };
    for (mut screen, material) in &mut screens {
        show_frame(time.elapsed_secs() - started, &mut screen, material, &mut materials, &mut images);
    }
}

fn show_frame(
    seconds: f32,
    player: &mut Screen,
    material: &MeshMaterial3d<StandardMaterial>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) {
    let clip = &player.clip;
    let t = seconds % clip.seconds();
    let index = ((t * clip.fps) as usize).min(clip.frames.len() - 1);
    if player.shown == Some(index) {
        return;
    }
    let Ok(decoded) = image::load_from_memory(clip.frames[index]) else { return };
    let rgba = decoded.to_rgba8();
    let (w, h) = rgba.dimensions();
    let frame = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    let slot = player.next_image;
    let handle = player.images[slot].clone();
    let _ = images.insert(&handle, frame);
    if let Some(m) = materials.get_mut(&material.0) {
        m.base_color_texture = Some(handle);
    }
    player.next_image = (slot + 1) % player.images.len();
    player.shown = Some(index);
}

/// Points as the scoreboard writes them: 12.340.
fn points(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// What the scoreboard says: alone, this player's total and best line; online, everyone ranked by total.
fn score_lines(score: &super::score::Score, net: Option<&crate::multiplayer::Multiplayer>) -> Vec<String> {
    let players = net.map_or_else(Vec::new, crate::multiplayer::Multiplayer::names);
    if players.len() < 2 {
        return vec![
            "PLACAR".into(),
            format!("TOTAL  {}", points(score.total())),
            format!("MELHOR LINHA  {}", points(score.best())),
        ];
    }
    let net = net.unwrap();
    let mut ranked: Vec<(String, i64)> = players
        .into_iter()
        .map(|(id, name, me)| {
            let total = if me {
                score.total()
            } else {
                net.record_of(id, crate::multiplayer::SCORE_KEY)
                    .and_then(|v| v.get(..8))
                    .map_or(0, |b| i64::from_le_bytes(b.try_into().unwrap()))
            };
            (if me { format!("{name} (voce)") } else { name }, total)
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    let mut out = vec!["QUEM ESTA NA FRENTE".to_owned()];
    out.extend(ranked.iter().take(SCORE_ROWS).enumerate().map(|(i, (name, total))| format!("{}. {name}   {}", i + 1, points(*total))));
    out
}

/// Every so often the big screen shows the scores; online, this player's points go out for everyone's ranking.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn scoreboard(
    time: Res<Time<Real>>,
    player: Option<Res<Player>>,
    score: Option<Res<super::score::Score>>,
    net: Option<ResMut<crate::multiplayer::Multiplayer>>,
    textures: Option<Res<MarcoTextures>>,
    mut overlay: Query<&mut Visibility, With<ScoreOverlay>>,
    lines: Query<(&ScoreLine, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut shown: Local<Vec<String>>,
) {
    let (Some(score), Some(textures)) = (score, textures) else { return };
    let mut net = net;
    if let Some(net) = net.as_mut().filter(|n| n.active()) {
        let mut value = score.total().to_le_bytes().to_vec();
        value.extend(score.best().to_le_bytes());
        net.publish_record(crate::multiplayer::SCORE_KEY, value);
    }
    let Ok(mut visibility) = overlay.single_mut() else { return };
    let on = player.and_then(|p| p.started).is_some_and(|started| {
        (time.elapsed_secs() - started) % (VIDEO_SECONDS + SCORES_SECONDS) >= VIDEO_SECONDS
    });
    let want = if on { Visibility::Inherited } else { Visibility::Hidden };
    if *visibility != want {
        *visibility = want;
    }
    if !on {
        return;
    }
    let text = score_lines(&score, net.as_deref());
    shown.resize(SCORE_ROWS + 1, String::new());
    for (line, mesh) in &lines {
        let t = text.get(line.0).cloned().unwrap_or_default();
        if shown[line.0] != t {
            if let Some(target) = meshes.get_mut(&mesh.0) {
                *target = textures.font_metrics.text_mesh(&t, if line.0 == 0 { 0.75 } else { 0.6 });
            }
            shown[line.0] = t;
        }
    }
}

/// Each panel moves on to another thumbnail every few seconds.
fn cycle_panels(
    time: Res<Time<Real>>,
    textures: Option<Res<MarcoTextures>>,
    mut clock: Local<(f32, usize)>,
    panels: Query<(&Panel, &Children)>,
    pictures: Query<&MeshMaterial3d<StandardMaterial>, Without<Panel>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(textures) = textures else { return };
    clock.0 += time.delta_secs();
    if clock.0 < PANEL_SLIDE_SECONDS {
        return;
    }
    clock.0 = 0.0;
    clock.1 += 1;
    let count = textures.tv_slides.len();
    for (panel, children) in &panels {
        // The picture is the last child (after the two bezel boxes).
        let Some(material) = children.iter().last().and_then(|c| pictures.get(c).ok()) else { continue };
        if let Some(m) = materials.get_mut(&material.0) {
            m.base_color_texture = Some(textures.tv_slides[(panel.0 * 4 + clock.1) % count].clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_read_like_a_scoreboard() {
        assert_eq!(points(0), "0");
        assert_eq!(points(950), "950");
        assert_eq!(points(12340), "12.340");
        assert_eq!(points(1234567), "1.234.567");
        assert_eq!(points(-2500), "-2.500");
    }

    #[test]
    fn the_packed_clips_decode_and_any_sound_lasts_as_long() {
        // An empty pack (no clip yet) is fine: the screen shows "em breve".
        let Some(clip) = Clip::parse(FRAMES) else {
            assert!(AUDIO.is_empty());
            return;
        };
        assert!(clip.frames.len() > 10);
        for frame in [clip.frames[0], clip.frames[clip.frames.len() - 1]] {
            let image = image::load_from_memory(frame).expect("JPEG frame decodes");
            assert_eq!((image.width(), image.height()), (640, 360));
        }
        // silent (packed with --mute) or as long as the pictures
        let audio_seconds = AUDIO.len() as f32 / 2.0 / AUDIO_RATE as f32;
        assert!(AUDIO.is_empty() || (audio_seconds - clip.seconds()).abs() < 0.1);
        if let Some(reels) = Clip::parse(REELS) {
            let image = image::load_from_memory(reels.frames[0]).expect("JPEG frame decodes");
            assert_eq!((image.width(), image.height()), (360, 640));
        }
    }
}
