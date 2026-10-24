//! Marco VR dressing for the Quest build: the skater wears Marco Pereira's
//! galaxy jersey or black MarcoVR tee, tattoo sleeves, beard and a white Quest 3; placed pieces
//! get wood/concrete/metal textures and MarcoVR stickers (`build`); a statue
//! piece shows Marco on a pedestal; and help cards explain the controls.
//!
//! Every texture is original (`tools/make_marcovr_assets.py`) and embedded.
//! Two skins (galaxy jersey, black tee) are switched from the menu; see
//! [`wear_skin`].
use super::actions::Hand;
use super::build::BuildMode;
use super::rig::Rig;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::{Affine3A, Vec3A};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy_mod_xr::camera::XrCamera;
use std::f32::consts::FRAC_PI_2;

/// Width over height of `logo_sticker.png`.
pub(crate) const STICKER_ASPECT: f32 = 512.0 / 242.0;
/// Real-world size of the wrist help card, metres (texture is 1024x640).
const CARD_WIDTH_M: f32 = 0.19;
/// The top of the left controller faces the eyes past this (cosine).
const WRIST_FACING: f32 = 0.7;
/// Statue figure, game metres per stock skater metre, before the piece size.
pub(crate) const STATUE_FIGURE_SCALE: f32 = 1.3;

#[derive(Resource)]
pub(crate) struct MarcoTextures {
    pub sticker: Handle<Image>,
    pub plywood: Handle<Image>,
    pub concrete: Handle<Image>,
    pub metal: Handle<Image>,
    pub marble: Handle<Image>,
    pub help_skate: Handle<Image>,
    pub help_build: Handle<Image>,
    pub menu_panel: Handle<Image>,
    pub menu_select: Handle<Image>,
    pub badge_on: Handle<Image>,
    pub badge_off: Handle<Image>,
    pub badge_maps: [Handle<Image>; 5],
    pub badge_skins: [Handle<Image>; SKINS],
    pub font: Handle<Image>,
    pub score_board: Handle<Image>,
    pub move_hint: Handle<Image>,
    pub vip_coin: Handle<Image>,
    pub tv_header: Handle<Image>,
    pub tv_soon: Handle<Image>,
    /// The channel's thumbnails, for the park's big screen.
    pub tv_slides: [Handle<Image>; 16],
    pub font_metrics: FontAtlas,
    /// Per skin (galaxy, black): the top, and the arm overlay.
    shirts: [Image; SKINS],
    arms: [Image; SKINS],
    face: Image,
}

/// The whole mip chain after `base` (box filter), and the level count. The
/// Quest samples minified textures far faster, and they shimmer less.
fn mip_chain(base: &[u8], w: u32, h: u32) -> (Vec<u8>, u32) {
    let mut out = base.to_vec();
    let (mut pw, mut ph) = (w as usize, h as usize);
    let mut prev = base.to_vec();
    let mut levels = 1;
    while pw > 1 || ph > 1 {
        let (nw, nh) = ((pw / 2).max(1), (ph / 2).max(1));
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(pw - 1), (2 * y + dy).min(ph - 1));
                        sum += prev[(sy * pw + sx) * 4 + c] as u32;
                    }
                    next[(y * nw + x) * 4 + c] = (sum / 4) as u8;
                }
            }
        }
        out.extend_from_slice(&next);
        prev = next;
        (pw, ph) = (nw, nh);
        levels += 1;
    }
    (out, levels)
}

fn decode(bytes: &[u8], repeat: bool) -> Image {
    let rgba = image::load_from_memory(bytes)
        .expect("embedded Marco VR texture decodes")
        .to_rgba8();
    let (w, h) = rgba.dimensions();
    let (data, levels) = mip_chain(rgba.as_raw(), w, h);
    let mut out = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    out.data = Some(data);
    out.texture_descriptor.mip_level_count = levels;
    out.sampler = ImageSampler::Descriptor(if repeat {
        ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            ..ImageSamplerDescriptor::linear()
        }
    } else {
        ImageSamplerDescriptor::linear()
    });
    out
}

/// One level only: these are blended into the skater's own images on the CPU.
fn decode_flat(bytes: &[u8]) -> Image {
    let mut image = decode(bytes, false);
    let size = (image.width() * image.height() * 4) as usize;
    if let Some(data) = image.data.as_mut() {
        data.truncate(size);
    }
    image.texture_descriptor.mip_level_count = 1;
    image
}

/// The glyph atlas `font.png` (ASCII 32..126 in Poppins Bold, white with a
/// black outline) and its advances, from `font.json`.
#[derive(Clone)]
pub(crate) struct FontAtlas {
    first: u32,
    cols: u32,
    cell: Vec2,
    pad: f32,
    advance: Vec<f32>,
    size: Vec2,
}

impl FontAtlas {
    fn load() -> Self {
        let json: serde_json::Value =
            serde_json::from_str(include_str!("marcovr/font.json")).expect("embedded font.json parses");
        let num = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0) as f32;
        let advance: Vec<f32> = json["advance"].as_array().map(|a| a.iter().map(num).collect()).unwrap_or_default();
        let cols = json["cols"].as_u64().unwrap_or(16) as u32;
        let cell = Vec2::new(num(&json["cell"][0]), num(&json["cell"][1]));
        let rows = (advance.len() as u32).div_ceil(cols.max(1));
        Self {
            first: json["first"].as_u64().unwrap_or(32) as u32,
            cols,
            cell,
            pad: num(&json["pad"]),
            size: Vec2::new(cell.x * cols as f32, cell.y * rows as f32),
            advance,
        }
    }

    /// A centred line of text facing +Z, `height` units tall.
    pub(crate) fn text_mesh(&self, text: &str, height: f32) -> Mesh {
        let k = height / self.cell.y;
        let glyph = |c: char| {
            let i = (c as u32).wrapping_sub(self.first) as usize;
            if i < self.advance.len() { i } else { ('?' as u32 - self.first) as usize }
        };
        let width: f32 = text.chars().map(|c| self.advance[glyph(c)] * k).sum();
        let mut x = -width * 0.5;
        let (mut positions, mut uvs, mut indices) = (Vec::new(), Vec::new(), Vec::new());
        for c in text.chars() {
            let i = glyph(c);
            if c != ' ' {
                let (col, row) = ((i as u32 % self.cols) as f32, (i as u32 / self.cols) as f32);
                let (u0, v0) = (col * self.cell.x / self.size.x, row * self.cell.y / self.size.y);
                let (u1, v1) = (u0 + self.cell.x / self.size.x, v0 + self.cell.y / self.size.y);
                let (x0, x1) = (x - self.pad * k, x - self.pad * k + self.cell.x * k);
                let (y0, y1) = (-height * 0.5, height * 0.5);
                let base = positions.len() as u32;
                positions.extend([[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]]);
                uvs.extend([[u0, v1], [u1, v1], [u1, v0], [u0, v0]]);
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            x += self.advance[i] * k;
        }
        let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
        Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_indices(bevy::mesh::Indices::U32(indices))
    }
}

macro_rules! texture {
    ($name:literal) => {
        include_bytes!(concat!("marcovr/", $name))
    };
}

/// Startup: decode the embedded textures. Runs before the build plugin
/// spawns any piece.
pub(crate) fn load_textures(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut add = |bytes: &[u8], repeat: bool| images.add(decode(bytes, repeat));
    let textures = MarcoTextures {
        sticker: add(texture!("logo_sticker.png"), false),
        plywood: add(texture!("plywood.png"), true),
        concrete: add(texture!("concrete.png"), true),
        metal: add(texture!("metal.png"), true),
        marble: add(texture!("marble.png"), true),
        help_skate: add(texture!("help_skate.png"), false),
        help_build: add(texture!("help_build.png"), false),
        menu_panel: add(texture!("menu_panel.png"), false),
        menu_select: add(texture!("menu_select.png"), false),
        badge_on: add(texture!("badge_on.png"), false),
        badge_off: add(texture!("badge_off.png"), false),
        badge_maps: [
            add(texture!("badge_map0.png"), false),
            add(texture!("badge_map1.png"), false),
            add(texture!("badge_map2.png"), false),
            add(texture!("badge_map3.png"), false),
            add(texture!("badge_map4.png"), false),
        ],
        badge_skins: [add(texture!("badge_skin0.png"), false), add(texture!("badge_skin1.png"), false)],
        font: add(texture!("font.png"), false),
        score_board: add(texture!("score_board.png"), false),
        move_hint: add(texture!("move_hint.png"), false),
        vip_coin: add(texture!("vip_coin.png"), false),
        tv_header: add(texture!("tv_header.png"), false),
        tv_soon: add(texture!("tv_soon.png"), false),
        tv_slides: [
            add(texture!("tv/00.jpg"), false),
            add(texture!("tv/01.jpg"), false),
            add(texture!("tv/02.jpg"), false),
            add(texture!("tv/03.jpg"), false),
            add(texture!("tv/04.jpg"), false),
            add(texture!("tv/05.jpg"), false),
            add(texture!("tv/06.jpg"), false),
            add(texture!("tv/07.jpg"), false),
            add(texture!("tv/08.jpg"), false),
            add(texture!("tv/09.jpg"), false),
            add(texture!("tv/10.jpg"), false),
            add(texture!("tv/11.jpg"), false),
            add(texture!("tv/12.jpg"), false),
            add(texture!("tv/13.jpg"), false),
            add(texture!("tv/14.jpg"), false),
            add(texture!("tv/15.jpg"), false),
        ],
        font_metrics: FontAtlas::load(),
        shirts: [decode_flat(texture!("shirt_galaxy.png")), decode_flat(texture!("shirt_black.png"))],
        arms: [decode_flat(texture!("arm_galaxy.png")), decode_flat(texture!("arm_black.png"))],
        face: decode_flat(texture!("face_overlay.png")),
    };
    commands.insert_resource(textures);
}

pub(crate) struct MarcoPlugin;

impl Plugin for MarcoPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MarcoSkin>()
            .add_systems(Startup, (spawn_help, load_skin))
            // Before the retail character shader binds the stock materials.
            .add_systems(PreUpdate, (wear_skin, wear_remote_skins))
            .add_systems(Update, (fit_headsets, pose_statues, show_help));
    }
}

// ------------------------------------------------------------------ outfit

/// Marco's two looks: his galaxy jersey (the default) and the black MarcoVR
/// tee. Saved next to the layout.
#[derive(Resource, Default)]
pub(crate) struct MarcoSkin {
    pub skin: usize,
    /// The skin the shared images were last painted for.
    painted: Option<usize>,
}

pub(crate) const SKINS: usize = 2;

impl MarcoSkin {
    pub(crate) fn next(&mut self) {
        self.skin = (self.skin + 1) % SKINS;
    }
}

fn settings_path(config: &crate::config::Config) -> std::path::PathBuf {
    config.asset_root.parent().unwrap_or(&config.asset_root).join("marco-settings.json")
}

fn load_skin(config: Res<crate::config::Config>, mut skin: ResMut<MarcoSkin>) {
    let saved = std::fs::read(settings_path(&config))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["skin"].as_u64());
    skin.skin = saved.map_or(0, |s| s as usize % SKINS);
}

/// The skater's pieces get their own materials (clones of the stock ones),
/// swapped onto the entities before the retail character shader binds the
/// stock ones; that is what makes the outfit stick. The top and arms have
/// one image per skin, and changing skin points their clones at the other
/// image: editing the material is what makes the renderer rebind it (an
/// image changed in place keeps showing the old one).
#[derive(Default)]
struct Wardrobe {
    torso: Option<[Handle<Image>; SKINS]>,
    arm: Option<[Handle<Image>; SKINS]>,
    torso_clones: Vec<Handle<StandardMaterial>>,
    arm_clones: Vec<Handle<StandardMaterial>>,
    /// Stock material -> its Marco clone.
    clones: bevy::platform::collections::HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
}

#[derive(Component)]
struct Dressed;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn wear_skin(
    mut commands: Commands,
    mut wardrobe: Local<Wardrobe>,
    mut skin: ResMut<MarcoSkin>,
    config: Res<crate::config::Config>,
    textures: Option<Res<MarcoTextures>>,
    pieces: Query<(Entity, &bevy::gltf::GltfMaterialName, &MeshMaterial3d<StandardMaterial>), Without<Dressed>>,
    parents: Query<&ChildOf>,
    owners: Query<(), Or<(With<crate::world::PlayerRoot>, With<StatueFigure>)>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(textures) = textures else { return };
    for (entity, name, handle) in &pieces {
        if !parents.iter_ancestors(entity).any(|e| owners.contains(e)) {
            continue;
        }
        commands.entity(entity).insert(Dressed);
        let Some(stock) = materials.get(&handle.0).cloned() else { continue };
        let clone = if let Some(clone) = wardrobe.clones.get(&handle.0.id()) {
            clone.clone()
        } else {
            let dressed = outfit(&name.0, &stock, skin.skin, &mut wardrobe, &textures, &mut images);
            let clone = materials.add(dressed);
            match name.0.as_str() {
                "Retail_OuterTorso" => wardrobe.torso_clones.push(clone.clone()),
                "Retail_Arm" => wardrobe.arm_clones.push(clone.clone()),
                _ => {}
            }
            wardrobe.clones.insert(handle.0.id(), clone.clone());
            info!("MARCOVR skin: dressed {}", name.0);
            clone
        };
        commands.entity(entity).insert(MeshMaterial3d(clone));
        // Repaint once the new images exist.
        skin.painted = None;
    }
    if skin.painted == Some(skin.skin) || wardrobe.torso.is_none() {
        return;
    }
    let s = skin.skin % SKINS;
    for (clones, per_skin) in [(&wardrobe.torso_clones, &wardrobe.torso), (&wardrobe.arm_clones, &wardrobe.arm)] {
        let Some(per_skin) = per_skin else { continue };
        for clone in clones {
            if let Some(material) = materials.get_mut(clone) {
                material.base_color_texture = Some(per_skin[s].clone());
            }
        }
    }
    info!("MARCOVR skin: {}", ["galaxia", "preta"][s]);
    let saved = serde_json::json!({ "skin": s });
    if let Err(e) = std::fs::write(settings_path(&config), saved.to_string()) {
        warn!("MARCOVR could not save the skin: {e}");
    }
    skin.painted = Some(s);
}

/// A remote skater's piece dressed in their Marco outfit: the material it had, and the skin it wears now.
#[derive(Component)]
struct RemoteDress {
    stock: Handle<StandardMaterial>,
    skin: usize,
}

/// The pieces the outfit changes.
const OUTFIT_PIECES: [&str; 5] = ["Retail_OuterTorso", "Retail_Arm", "Retail_Rostral", "Retail_Pants", "Retail_Hair"];

/// Online, every player publishes their outfit and each remote skater is dressed in theirs, changing with it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn wear_remote_skins(
    mut commands: Commands,
    mut wardrobe: Local<Wardrobe>,
    mut clones: Local<bevy::platform::collections::HashMap<(AssetId<StandardMaterial>, usize), Handle<StandardMaterial>>>,
    skin: Res<MarcoSkin>,
    net: Option<ResMut<crate::multiplayer::Multiplayer>>,
    textures: Option<Res<MarcoTextures>>,
    remotes: Query<(Entity, &crate::multiplayer::RemoteActor)>,
    children: Query<&Children>,
    pieces: Query<(&bevy::gltf::GltfMaterialName, &MeshMaterial3d<StandardMaterial>, Option<&RemoteDress>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(mut net), Some(textures)) = (net, textures) else { return };
    if !net.active() {
        return;
    }
    net.publish_record(crate::multiplayer::SKIN_KEY, vec![skin.skin as u8]);
    for (root, actor) in &remotes {
        let want = net
            .record_of(actor.0, crate::multiplayer::SKIN_KEY)
            .and_then(|v| v.first())
            .map_or(0, |&s| s as usize % SKINS);
        for piece in children.iter_descendants(root) {
            let Ok((name, material, dress)) = pieces.get(piece) else { continue };
            if !OUTFIT_PIECES.contains(&name.0.as_str()) {
                continue;
            }
            // dressed already, unless the skin changed or something else swapped the material since
            let stock = match dress {
                Some(d) if clones.get(&(d.stock.id(), d.skin)) == Some(&material.0) => {
                    if d.skin == want {
                        continue;
                    }
                    d.stock.clone()
                }
                _ => material.0.clone(),
            };
            let key = (stock.id(), want);
            let clone = match clones.get(&key) {
                Some(c) => c.clone(),
                None => {
                    let Some(base) = materials.get(&stock).cloned() else { continue };
                    let c = materials.add(outfit(&name.0, &base, want, &mut wardrobe, &textures, &mut images));
                    clones.insert(key, c.clone());
                    c
                }
            };
            commands.entity(piece).insert((MeshMaterial3d(clone), RemoteDress { stock, skin: want }));
        }
    }
}

/// One piece of the skater dressed as Marco in `skin` (the stock material for pieces the outfit leaves alone).
fn outfit(
    name: &str,
    stock: &StandardMaterial,
    skin: usize,
    wardrobe: &mut Wardrobe,
    textures: &MarcoTextures,
    images: &mut Assets<Image>,
) -> StandardMaterial {
    let mut dressed = stock.clone();
    let source = stock.base_color_texture.as_ref().and_then(|h| images.get(h)).cloned();
    match name {
        "Retail_OuterTorso" => {
            let shirts = wardrobe
                .torso
                .get_or_insert_with(|| std::array::from_fn(|i| images.add(textures.shirts[i].clone())));
            dressed.base_color_texture = Some(shirts[skin % SKINS].clone());
            dressed.base_color = Color::WHITE;
        }
        "Retail_Arm" => {
            let stock_arm = source.filter(rgba8).unwrap_or_else(|| skin_tone(&textures.arms[0]));
            let arms = wardrobe.arm.get_or_insert_with(|| {
                std::array::from_fn(|i| {
                    let mut painted = stock_arm.clone();
                    blend(&mut painted, &textures.arms[i]);
                    images.add(painted)
                })
            });
            dressed.base_color_texture = Some(arms[skin % SKINS].clone());
        }
        "Retail_Rostral" => {
            if let Some(mut face) = source.filter(rgba8) {
                blend(&mut face, &textures.face);
                dressed.base_color_texture = Some(images.add(face));
            }
        }
        // Dark jeans.
        "Retail_Pants" => {
            let c = stock.base_color.to_srgba();
            dressed.base_color = Color::srgba(c.red * 0.32, c.green * 0.36, c.blue * 0.46, c.alpha);
        }
        // Short dark hair. Its cards cut out almost everything under
        // the plain material (coverage lives in a second texture), so
        // it is made solid.
        "Retail_Hair" => {
            dressed.alpha_mode = AlphaMode::Opaque;
            dressed.base_color = Color::srgb(0.07, 0.05, 0.04);
        }
        _ => {}
    }
    dressed
}

/// When the stock arm image can't be read: a plain skin tone under the
/// sleeves and tattoos.
fn skin_tone(like: &Image) -> Image {
    let mut image = like.clone();
    if let Some(data) = image.data.as_mut() {
        for px in data.chunks_exact_mut(4) {
            px.copy_from_slice(&[214, 168, 140, 255]);
        }
    }
    image
}

fn rgba8_format(format: TextureFormat) -> bool {
    matches!(format, TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm)
}

fn rgba8(image: &Image) -> bool {
    rgba8_format(image.texture_descriptor.format) && image.data.is_some()
}

/// Alpha-blend `overlay` over `target`, sampling it by UV (sizes may differ).
fn blend(target: &mut Image, overlay: &Image) -> bool {
    if !rgba8(target) {
        return false;
    }
    let (tw, th) = (target.width() as usize, target.height() as usize);
    let (ow, oh) = (overlay.width() as usize, overlay.height() as usize);
    let (Some(dst), Some(src)) = (target.data.as_mut(), overlay.data.as_ref()) else {
        return false;
    };
    for y in 0..th {
        let oy = y * oh / th;
        for x in 0..tw {
            let o = (oy * ow + x * ow / tw) * 4;
            let a = src[o + 3] as u32;
            if a == 0 {
                continue;
            }
            let d = (y * tw + x) * 4;
            for c in 0..3 {
                dst[d + c] = ((dst[d + c] as u32 * (255 - a) + src[o + c] as u32 * a) / 255) as u8;
            }
        }
    }
    true
}


// ------------------------------------------------------------------ Quest 3

/// The stock skater's HEAD inverse bind matrix: model space -> HEAD bone
/// space. Children of a frame with this transform are placed in model
/// metres (+Y up, the face towards +Z).
const HEAD_FROM_MODEL: [f32; 16] = [
    0.0, -1.0, 0.0, 0.0, //
    0.998, 0.0, -0.0635, 0.0, //
    0.0635, 0.0, 0.998, 0.0, //
    -1.5811, 0.0, 0.1084, 1.0,
];

#[derive(Component)]
struct HeadsetFitted;

#[derive(Default)]
struct HeadsetParts(Option<Vec<(Handle<Mesh>, Handle<StandardMaterial>, Transform)>>);

/// A white Quest 3 on every skater-rig HEAD bone (the player and statues).
fn fit_headsets(
    mut commands: Commands,
    mut parts: Local<HeadsetParts>,
    heads: Query<(Entity, &Name), (Added<Name>, Without<HeadsetFitted>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, name) in &heads {
        if name.as_str() != "HEAD" {
            continue;
        }
        let parts = parts.0.get_or_insert_with(|| headset_parts(&mut meshes, &mut materials));
        commands.entity(entity).insert(HeadsetFitted).with_children(|head| {
            head.spawn((
                Name::new("Quest 3"),
                Transform::from_matrix(Mat4::from_cols_array(&HEAD_FROM_MODEL)),
                Visibility::default(),
            ))
            .with_children(|frame| {
                for (mesh, material, transform) in parts.iter() {
                    frame.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), *transform));
                }
            });
        });
    }
}

fn headset_parts(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>, Transform)> {
    let shell = materials.add(StandardMaterial {
        base_color: Color::srgb(0.94, 0.94, 0.93),
        perceptual_roughness: 0.45,
        ..default()
    });
    let face = materials.add(StandardMaterial {
        base_color: Color::srgb(0.98, 0.98, 0.97),
        perceptual_roughness: 0.3,
        ..default()
    });
    let black = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.03, 0.035),
        perceptual_roughness: 0.2,
        ..default()
    });
    let strap = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.85, 0.85),
        perceptual_roughness: 0.8,
        ..default()
    });
    // Visor centred in front of the eyes, model metres.
    let centre = Vec3::new(0.0, 1.655, 0.118);
    let mut out = vec![
        (meshes.add(Cuboid::new(0.17, 0.085, 0.06)), shell.clone(), Transform::from_translation(centre)),
        (
            meshes.add(Cuboid::new(0.158, 0.074, 0.012)),
            face,
            Transform::from_translation(centre + Vec3::Z * 0.033),
        ),
        // Rounded ends.
        (
            meshes.add(Cylinder::new(0.0425, 0.06)),
            shell.clone(),
            Transform::from_translation(centre + Vec3::X * 0.085).with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
        ),
        (
            meshes.add(Cylinder::new(0.0425, 0.06)),
            shell,
            Transform::from_translation(centre - Vec3::X * 0.085).with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
        ),
        // Head strap around the skull, a little lower at the front.
        (
            meshes.add(Torus::new(0.1, 0.112)),
            strap,
            Transform::from_xyz(0.0, 1.69, -0.005)
                .with_rotation(Quat::from_rotation_x(-0.2))
                .with_scale(Vec3::new(0.9, 1.0, 1.0)),
        ),
    ];
    // Marco's quiff, sweeping up and forward over the strap.
    let hair = materials.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.05, 0.04),
        perceptual_roughness: 0.7,
        ..default()
    });
    let tuft = meshes.add(Cone::new(0.03, 0.08));
    for (x, lean, size) in [(-0.035, 0.5, 0.85), (0.0, 0.7, 1.0), (0.035, 0.55, 0.9), (0.015, 0.95, 0.8)] {
        out.push((
            tuft.clone(),
            hair.clone(),
            Transform::from_xyz(x, 1.765, 0.035 + 0.03 * lean)
                .with_rotation(Quat::from_rotation_x(lean) * Quat::from_rotation_z(-x * 6.0))
                .with_scale(Vec3::splat(size)),
        ));
    }
    // Quest 3's three pill-shaped sensor clusters.
    let pill = meshes.add(Capsule3d::new(0.0075, 0.016));
    for x in [-0.05, 0.0, 0.05] {
        out.push((
            pill.clone(),
            black.clone(),
            Transform::from_translation(centre + Vec3::new(x, 0.0, 0.04)),
        ));
    }
    out
}

// ------------------------------------------------------------------ statue

/// The figure on a statue piece: a second, unanimated copy of the skater.
#[derive(Component)]
pub(crate) struct StatueFigure;

#[derive(Component)]
struct Posed;

pub(crate) fn statue_scene(server: &AssetServer, manifest: &crate::assets::AssetManifest) -> Handle<Scene> {
    server.load(GltfAssetLabel::Scene(0).from_asset(manifest.0.character_scene.clone()))
}

/// Marco's signature pose: right arm up pointing at the sky, left hand on hip.
fn pose_statues(
    mut commands: Commands,
    figures: Query<Entity, (With<StatueFigure>, Without<Posed>)>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    mut transforms: Query<&mut Transform>,
) {
    for figure in &figures {
        let find = |bone: &str| {
            children
                .iter_descendants(figure)
                .find(|&e| names.get(e).is_ok_and(|n| n.as_str() == bone))
        };
        let bones = [
            ("RIGHTARM", "RIGHTFOREARM", Vec3::new(-0.3, 0.94, 0.12)),
            ("RIGHTFOREARM", "RIGHTHAND", Vec3::new(-0.08, 1.0, 0.1)),
            ("LEFTARM", "LEFTFOREARM", Vec3::new(0.55, -0.8, -0.1)),
            ("LEFTFOREARM", "LEFTHAND", Vec3::new(-0.45, -0.55, 0.45)),
        ];
        let Some(found) = bones
            .iter()
            .map(|&(bone, child, dir)| Some((find(bone)?, find(child)?, dir.normalize())))
            .collect::<Option<Vec<_>>>()
        else {
            continue; // Scene not spawned yet.
        };
        for (bone, child, target) in found {
            let model = |e: Entity, transforms: &Query<&mut Transform>| {
                let mut affine = Affine3A::IDENTITY;
                let mut at = e;
                while at != figure {
                    let Ok(t) = transforms.get(at) else { break };
                    affine = t.compute_affine() * affine;
                    let Ok(parent) = parents.get(at) else { break };
                    at = parent.parent();
                }
                affine
            };
            let (b, c) = (model(bone, &transforms), model(child, &transforms));
            let from = (c.translation - b.translation).normalize_or_zero();
            if from == Vec3A::ZERO {
                continue;
            }
            let delta = Quat::from_rotation_arc(from.into(), target);
            let (_, global, _) = b.to_scale_rotation_translation();
            if let Ok(mut local) = transforms.get_mut(bone) {
                local.rotation = (local.rotation * global.inverse() * delta * global).normalize();
            }
        }
        commands.entity(figure).insert(Posed);
    }
}

// ------------------------------------------------------------------ help cards

#[derive(Component)]
enum HelpCard {
    Skate,
    Build,
}

/// Head pose in tracking space: between the eye cameras, looking their way.
pub(crate) fn head_pose<'a>(eyes: impl Iterator<Item = &'a Transform>) -> Option<Transform> {
    let mut head = None::<Transform>;
    let mut count = 0.0;
    for eye in eyes {
        let h = head.get_or_insert(Transform::from_rotation(eye.rotation).with_translation(Vec3::ZERO));
        h.translation += eye.translation;
        count += 1.0;
    }
    head.map(|mut h| {
        h.translation /= count;
        h
    })
}

fn spawn_help(
    mut commands: Commands,
    textures: Option<Res<MarcoTextures>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(textures) = textures else { return };
    let mesh = meshes.add(Rectangle::new(CARD_WIDTH_M, CARD_WIDTH_M * 640.0 / 1024.0));
    for (kind, image) in [(HelpCard::Skate, &textures.help_skate), (HelpCard::Build, &textures.help_build)] {
        commands.spawn((
            kind,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(image.clone()),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            })),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

/// The wrist card shows while the top of the left controller faces the eyes
/// (and the menu is closed): skating controls, or building ones in build mode.
#[allow(clippy::type_complexity)]
fn show_help(
    rig: Res<Rig>,
    mode: Res<BuildMode>,
    menu: Res<super::menu::XrMenu>,
    hands: Query<(&Hand, &Transform), Without<HelpCard>>,
    eyes: Query<&Transform, (With<XrCamera>, Without<HelpCard>)>,
    mut cards: Query<(&HelpCard, &mut Transform, &mut Visibility)>,
) {
    let Some(root) = rig.root_transform() else { return };
    let head = head_pose(eyes.iter());
    // Lying on the right of the left controller's top face, its top edge
    // towards the controller's front.
    let left = hands.iter().find(|(h, _)| matches!(h, Hand::Left)).map(|(_, t)| *t);
    let wrist = left.zip(head).filter(|_| !menu.open).and_then(|(left, head)| {
        let up = left.rotation * Vec3::Y;
        let to_eye = (head.translation - left.translation).normalize_or_zero();
        (up.dot(to_eye) > WRIST_FACING).then(|| Transform {
            translation: left.translation + up * 0.03 + left.rotation * Vec3::new(0.11, 0.0, -0.02),
            rotation: left.rotation * Quat::from_rotation_x(-FRAC_PI_2),
            scale: Vec3::ONE,
        })
    });
    for (card, mut transform, mut visibility) in &mut cards {
        let pose = match card {
            HelpCard::Skate => wrist.filter(|_| !mode.active),
            HelpCard::Build => wrist.filter(|_| mode.active),
        };
        let shown = match pose {
            Some(pose) => {
                let target = root.mul_transform(pose);
                if *transform != target {
                    *transform = target;
                }
                Visibility::Visible
            }
            None => Visibility::Hidden,
        };
        if *visibility != shown {
            *visibility = shown;
        }
    }
}
