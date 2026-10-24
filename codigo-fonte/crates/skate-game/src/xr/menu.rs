//! The Marco VR menu: a panel that floats in front of the player. It opens at
//! every launch on the "how to play" page, and on a tap of the left menu
//! button (`touch`); holding that button a second is still build mode.
//!
//! Every option lives here: the Marco park (put it in front of you, which of
//! the four maps, grab and carry it somewhere else, its size, its barrier,
//! take it away), clearing every placed piece, Marco's outfit, the score
//! board's spot, build mode, the room's detected surfaces and a fresh room
//! scan, and online rooms (`online`). While it is open, or the park is being
//! carried, the pad is neutral.
//!
//! The labels are drawn into `menu_panel.png` by
//! `tools/make_marcovr_assets.py`; the row layout below mirrors its numbers.
use super::actions::{Hand, LatestTouch, MenuTap, TouchRead};
use super::build::park::{MAPS, Park, START_SCALE};
use super::build::{BuildMode, MAX_SCALE, MIN_SCALE, PlacedPieces};
use super::marcovr::{MarcoSkin, MarcoTextures, SKINS, head_pose};
use super::plane_view::PlaneOverlay;
use super::planes::SceneCapture;
use super::rig::{Rig, heading};
use crate::input::ExternalPad;
use bevy::prelude::*;
use bevy_mod_openxr::openxr_session_running;
use bevy_mod_xr::camera::XrCamera;
use skate_core::input::xbox::XboxState;

/// Panel width in the room, metres; its texture is 1024 x 1280.
const PANEL_W_M: f32 = 0.56;
const PX: f32 = PANEL_W_M / 1024.0;
const PANEL_H_PX: f32 = 1280.0;
const ROW_TOP_PX: f32 = 215.0;
const ROW_H_PX: f32 = 66.0;
const BADGE_X_PX: f32 = 840.0;
const HELP_W_M: f32 = 0.7;
const DISTANCE_M: f32 = 0.7;
const FLICK: f32 = 0.6;
/// Each step of the size option, as a world scale factor.
const SIZE_STEP: f32 = 1.2;
/// Where a park put "in front of me" goes: ahead of and below the eyes, m.
const PARK_AHEAD_M: f32 = 0.75;
const PARK_BELOW_EYES_M: f32 = 0.6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Item {
    Play,
    Help,
    ParkHere,
    Map,
    MovePark,
    Size,
    Fence,
    RemovePark,
    ClearPieces,
    Skin,
    BoardHere,
    Build,
    Planes,
    Rescan,
    Online,
}

/// In the panel's order.
const ITEMS: [Item; 15] = [
    Item::Play,
    Item::Help,
    Item::ParkHere,
    Item::Map,
    Item::MovePark,
    Item::Size,
    Item::Fence,
    Item::RemovePark,
    Item::ClearPieces,
    Item::Skin,
    Item::BoardHere,
    Item::Build,
    Item::Planes,
    Item::Rescan,
    Item::Online,
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Page {
    Main,
    /// 0: skating controls, 1: building controls.
    Help(usize),
    Online,
}

#[derive(Resource)]
pub(crate) struct XrMenu {
    pub open: bool,
    /// The park follows the right hand while its grip is held.
    pub moving_park: bool,
    pub(super) page: Page,
    selected: usize,
    /// Tracking-space pose, set when it opens and the head is known.
    pub(super) pose: Option<Transform>,
    pub(super) online: super::online::Online,
}

impl Default for XrMenu {
    fn default() -> Self {
        Self {
            open: true,
            moving_park: false,
            page: Page::Help(0),
            selected: 0,
            pose: None,
            online: Default::default(),
        }
    }
}

#[derive(Default)]
struct Previous {
    a: bool,
    b: bool,
    up_down: i8,
    sideways: i8,
}

#[derive(Component, Clone, Copy, PartialEq)]
enum Part {
    Panel,
    Select,
    /// A row's state: on/off (1/0), the map, or the skin.
    Badge(Item, usize),
    Help(usize),
    MoveHint,
    /// The build's version, in the panel's top-right corner.
    Version,
}

/// The menu reads the controllers in this set; build mode runs after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct MenuInput;

pub(crate) struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<XrMenu>()
            .add_systems(
                Startup,
                (spawn_parts, super::online::spawn_parts).after(super::marcovr::load_textures),
            )
            .add_systems(
                PreUpdate,
                menu_input
                    .in_set(MenuInput)
                    .after(TouchRead)
                    .before(crate::input::poll_controllers)
                    .run_if(openxr_session_running),
            )
            .add_systems(
                Update,
                (place_park_in_front, show_parts, super::online::show_parts, super::online::name_tags),
            );
    }
}

fn flick(x: f32, was: i8) -> i8 {
    match x {
        x if x > FLICK => 1,
        x if x < -FLICK => -1,
        x if x.abs() < FLICK * 0.5 => 0,
        _ => was,
    }
}

#[allow(clippy::too_many_arguments)]
fn menu_input(
    touch: Res<LatestTouch>,
    tap: Res<MenuTap>,
    eyes: Query<&Transform, With<XrCamera>>,
    hands: Query<(&Hand, &Transform)>,
    mut menu: ResMut<XrMenu>,
    mut previous: Local<Previous>,
    mut grabbed: Local<Option<(Transform, Park)>>,
    mut pad: ResMut<ExternalPad>,
    mut mode: ResMut<BuildMode>,
    mut rig: ResMut<Rig>,
    mut placed: ResMut<PlacedPieces>,
    mut overlay: ResMut<PlaneOverlay>,
    mut capture: ResMut<SceneCapture>,
    mut skin: ResMut<MarcoSkin>,
    mut score: ResMut<super::score::Score>,
    mut net: Option<ResMut<crate::multiplayer::Multiplayer>>,
) {
    let t = &touch.0;
    let stick_y = if t.left_stick[1].abs() > t.right_stick[1].abs() { t.left_stick[1] } else { t.right_stick[1] };
    let stick_x = if t.left_stick[0].abs() > t.right_stick[0].abs() { t.left_stick[0] } else { t.right_stick[0] };
    let now = Previous {
        a: t.a || t.right_trigger > 0.7,
        b: t.b,
        up_down: flick(stick_y, previous.up_down),
        sideways: flick(stick_x, previous.sideways),
    };
    let was = std::mem::replace(&mut *previous, now);
    let now = &*previous;
    let a = now.a && !was.a;
    let b = now.b && !was.b;
    let up_down = (now.up_down != was.up_down).then_some(now.up_down).filter(|&d| d != 0);
    let sideways = (now.sideways != was.sideways).then_some(now.sideways).filter(|&d| d != 0);

    if menu.moving_park {
        pad.set(XboxState {
            buttons: 0,
            triggers: [0, 0],
            left: [0, 0],
            right: [0, 0],
        });
        let hand = hands.iter().find(|(h, _)| matches!(h, Hand::Right)).map(|(_, t)| *t);
        match (t.right_grip > 0.6, hand, *grabbed) {
            (true, Some(hand), None) => *grabbed = placed.park.map(|park| (hand, park)),
            (true, Some(hand), Some((from, park))) => {
                // Carry it rigidly with the hand, turning with it about +Y.
                let turn = heading(hand.rotation) - heading(from.rotation);
                let offset = Quat::from_rotation_y(turn) * (Vec3::from_array(park.position) - from.translation);
                placed.park = Some(Park {
                    position: (hand.translation + offset).to_array(),
                    yaw: park.yaw + turn,
                    ..park
                });
            }
            (false, _, Some(_)) => {
                *grabbed = None;
                let park = placed.park;
                placed.set_park(park);
                info!("MENU park dropped at {:?}", placed.park.map(|p| p.position));
            }
            _ => {}
        }
        if (a || b || tap.0) && grabbed.is_none() {
            menu.moving_park = false;
        }
        return;
    }
    if tap.0 {
        menu.open = !menu.open;
        if menu.open {
            menu.page = Page::Main;
            menu.pose = None;
        }
        info!("MENU open: {}", menu.open);
    }
    if !menu.open {
        return;
    }
    pad.set(XboxState {
        buttons: 0,
        triggers: [0, 0],
        left: [0, 0],
        right: [0, 0],
    });
    if menu.pose.is_none() {
        menu.pose = head_pose(eyes.iter()).map(panel_pose);
    }

    match menu.page {
        Page::Help(card) => {
            if let Some(d) = sideways {
                menu.page = Page::Help((card as i32 + d as i32).rem_euclid(2) as usize);
            }
            if a || b {
                menu.page = Page::Main;
            }
        }
        Page::Online => {
            if menu.online.input(net.as_deref_mut(), up_down, sideways, a, b) {
                menu.page = Page::Main;
            }
        }
        Page::Main => {
            if b {
                menu.open = false;
                return;
            }
            if let Some(d) = up_down {
                // Stick up is +1 and moves the highlight up the list.
                menu.selected = (menu.selected as i32 - d as i32).rem_euclid(ITEMS.len() as i32) as usize;
            }
            let item = ITEMS[menu.selected];
            let step = sideways.map(|d| d as i32).or(a.then_some(1));
            if let (Item::Map, Some(d)) = (item, step) {
                let next = (placed.map_choice as i32 + d).rem_euclid(MAPS.len() as i32) as usize;
                placed.map_choice = next;
                match placed.park {
                    Some(park) => placed.set_park(Some(Park { map: next, ..park })),
                    None => placed.place_in_front = true,
                }
                info!("MENU map {}", MAPS[next].name);
                return;
            }
            if let (Item::Skin, Some(_)) = (item, step) {
                skin.next();
                return;
            }
            if item == Item::Size {
                if let Some(d) = sideways {
                    // Right: a bigger park, so fewer game metres per real one.
                    let factor = if d > 0 { 1.0 / SIZE_STEP } else { SIZE_STEP };
                    rig.scale = (rig.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
                    let park = placed.park;
                    placed.set_park(park);
                    info!("MENU world scale {:.1}", rig.scale);
                }
            }
            if !a {
                return;
            }
            info!("MENU {item:?}");
            match item {
                Item::Play => menu.open = false,
                Item::Help => menu.page = Page::Help(0),
                Item::ParkHere => {
                    placed.place_in_front = true;
                    menu.open = false;
                }
                Item::MovePark => {
                    if placed.park.is_none() {
                        placed.place_in_front = true;
                    }
                    menu.moving_park = true;
                    menu.open = false;
                }
                Item::ClearPieces => placed.clear_pieces(),
                Item::Map | Item::Skin => {}
                Item::BoardHere => {
                    score.place_board_here = true;
                    menu.open = false;
                }
                Item::Size => {}
                Item::Fence => {
                    if let Some(park) = placed.park.as_mut() {
                        park.fence = !park.fence;
                        placed.revision += 1;
                    }
                }
                Item::RemovePark => placed.set_park(None),
                Item::Build => {
                    mode.active = true;
                    menu.open = false;
                }
                Item::Planes => overlay.always = !overlay.always,
                Item::Rescan => {
                    capture.requested = true;
                    menu.open = false;
                }
                Item::Online => menu.page = Page::Online,
            }
        }
    }
}

/// Facing the player, a little below eye level.
fn panel_pose(head: Transform) -> Transform {
    let forward = (head.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
    let at = head.translation + forward * DISTANCE_M - Vec3::Y * 0.08;
    Transform::from_translation(at).looking_to(forward, Vec3::Y)
}

/// The park asked for "in front of me" (the menu, or a first launch) goes
/// down as soon as the head is tracked: floating ahead, at about table height.
fn place_park_in_front(
    eyes: Query<&Transform, With<XrCamera>>,
    mut rig: ResMut<Rig>,
    mut placed: ResMut<PlacedPieces>,
) {
    if !placed.place_in_front {
        return;
    }
    let Some(head) = head_pose(eyes.iter()) else { return };
    placed.place_in_front = false;
    let forward = (head.rotation * Vec3::NEG_Z).with_y(0.0).normalize_or(Vec3::NEG_Z);
    let mut at = head.translation + forward * PARK_AHEAD_M;
    at.y = (head.translation.y - PARK_BELOW_EYES_M).max(0.05);
    let (fence, map) = placed.park.map_or((true, placed.map_choice), |p| (p.fence, p.map));
    if placed.park.is_none() {
        rig.scale = START_SCALE.clamp(MIN_SCALE, MAX_SCALE);
    }
    placed.set_park(Some(Park { position: at.to_array(), yaw: heading(head.rotation), fence, map }));
    info!("MENU Marco park in front at {at:?}, world scale {:.1}", rig.scale);
}

fn spawn_parts(
    mut commands: Commands,
    textures: Option<Res<MarcoTextures>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(textures) = textures else { return };
    let mut quad = |part: Part, image: &Handle<Image>, w: f32, h: f32| {
        commands.spawn((
            part,
            Mesh3d(meshes.add(Rectangle::new(w, h))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(image.clone()),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })),
            Transform::default(),
            Visibility::Hidden,
        ));
    };
    quad(Part::Panel, &textures.menu_panel, PANEL_W_M, PANEL_H_PX * PX);
    quad(Part::Select, &textures.menu_select, 944.0 * PX, 60.0 * PX);
    quad(Part::MoveHint, &textures.move_hint, 0.24, 0.24 * 150.0 / 1024.0);
    for item in [Item::Fence, Item::Planes] {
        quad(Part::Badge(item, 1), &textures.badge_on, 220.0 * PX, 52.0 * PX);
        quad(Part::Badge(item, 0), &textures.badge_off, 220.0 * PX, 52.0 * PX);
    }
    for (i, image) in textures.badge_maps.iter().enumerate() {
        quad(Part::Badge(Item::Map, i), image, 240.0 * PX, 52.0 * PX);
    }
    for (i, image) in textures.badge_skins.iter().enumerate().take(SKINS) {
        quad(Part::Badge(Item::Skin, i), image, 240.0 * PX, 52.0 * PX);
    }
    quad(Part::Help(0), &textures.help_skate, HELP_W_M, HELP_W_M * 640.0 / 1024.0);
    quad(Part::Help(1), &textures.help_build, HELP_W_M, HELP_W_M * 640.0 / 1024.0);
    commands.spawn((
        Part::Version,
        Mesh3d(meshes.add(textures.font_metrics.text_mesh(crate::version::LABEL, 0.016))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.494, 0.784, 0.949),
            base_color_texture: Some(textures.font.clone()),
            alpha_mode: AlphaMode::Mask(0.4),
            unlit: true,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Panel-local position (metres, +Z towards the player) of a row's centre.
fn row(item: Item, x_px: f32, z: f32) -> Vec3 {
    let i = ITEMS.iter().position(|&it| it == item).unwrap_or(0) as f32;
    let y_px = ROW_TOP_PX + i * ROW_H_PX + ROW_H_PX * 0.5;
    Vec3::new((x_px - 512.0) * PX, (PANEL_H_PX * 0.5 - y_px) * PX, z)
}

#[allow(clippy::type_complexity)]
fn show_parts(
    menu: Res<XrMenu>,
    rig: Res<Rig>,
    hands: Query<(&Hand, &Transform), Without<Part>>,
    eyes: Query<&Transform, (With<XrCamera>, Without<Part>)>,
    placed: Res<PlacedPieces>,
    overlay: Res<PlaneOverlay>,
    skin: Res<MarcoSkin>,
    mut parts: Query<(&Part, &mut Transform, &mut Visibility)>,
) {
    let root = rig.root_transform();
    // Over the right hand, facing the eyes, while the park is being carried.
    let hint = hands
        .iter()
        .find(|(h, _)| matches!(h, Hand::Right))
        .zip(head_pose(eyes.iter()))
        .map(|((_, hand), head)| {
            let at = hand.translation + Vec3::Y * 0.12;
            Transform::from_translation(at).looking_to((at - head.translation).with_y(0.0), Vec3::Y)
        });
    for (part, mut transform, mut visibility) in &mut parts {
        if *part == Part::MoveHint {
            let shown = match (hint.filter(|_| menu.moving_park), root) {
                (Some(pose), Some(root)) => {
                    *transform = root.mul_transform(pose);
                    Visibility::Visible
                }
                _ => Visibility::Hidden,
            };
            if *visibility != shown {
                *visibility = shown;
            }
            continue;
        }
        let local = match (*part, menu.page) {
            _ if !menu.open => None,
            (Part::Panel, Page::Main) => Some(Vec3::ZERO),
            (Part::Version, Page::Main) => Some(Vec3::new((880.0 - 512.0) * PX, (PANEL_H_PX * 0.5 - 40.0) * PX, 0.004)),
            (Part::Select, Page::Main) => Some(row(ITEMS[menu.selected], 512.0, 0.004)),
            (Part::Badge(item, value), Page::Main) => {
                let state = match item {
                    Item::Fence => placed.park.is_some_and(|p| p.fence) as usize,
                    Item::Map => placed.park.map_or(placed.map_choice, |p| p.map),
                    Item::Skin => skin.skin,
                    _ => overlay.always as usize,
                };
                (state == value).then(|| row(item, BADGE_X_PX, 0.006))
            }
            (Part::Help(card), Page::Help(shown)) => (card == shown).then_some(Vec3::ZERO),
            _ => None,
        };
        let shown = match (local, menu.pose, root) {
            (Some(local), Some(pose), Some(root)) => {
                let target = root.mul_transform(pose.mul_transform(Transform::from_translation(local)));
                if *transform != target {
                    *transform = target;
                }
                Visibility::Visible
            }
            _ => Visibility::Hidden,
        };
        if *visibility != shown {
            *visibility = shown;
        }
    }
}
