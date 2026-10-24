//! The Marco park drawn as a handful of merged meshes, one per material, in
//! the park's own frame: the Quest draws a dozen batches instead of a draw
//! call per piece, sticker, coping and prop. Rebuilt only when the park's
//! content changes (the barrier switch); every frame just moves its root.
//!
//! Besides the pieces it carries the scenery that makes the park feel alive:
//! planters with trees, benches, lit lamp posts, MarcoVR flags, a blue light
//! strip round the base and a painted ring round the statue.
use super::PlacedPieces;
use super::park::{Prop, local_parts, local_props};
use super::park_parts::Part;
use super::pieces::{PEDESTAL_HEIGHT, PieceKind, SLAB_SIZE, Surface, quad_normal};
use crate::xr::marcovr::{MarcoTextures, STATUE_FIGURE_SCALE, STICKER_ASPECT, StatueFigure};
use crate::xr::rig::Rig;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Texture repeat, game metres (same as loose pieces).
const TILE: f32 = 1.2;
const MARCO_BLUE: Color = Color::srgb(0.494, 0.784, 0.949);

/// One merged mesh's material.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Paint {
    Surface(Surface),
    /// Ramp side and back walls, teal.
    RampSide,
    /// Flat decks on top of ramps and boxes, a darker orange.
    Deck,
    /// The base's sides: the tray's metal frame.
    Tray,
    Sticker,
    Coping,
    Leaves,
    Bark,
    Soil,
    BenchWood,
    DarkMetal,
    Glow,
    LightStrip,
    Ring,
}

#[derive(Default)]
struct Buffer {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Buffer {
    fn quad(&mut self, corners: [Vec3; 4], uvs: [[f32; 2]; 4], normal: Vec3) {
        let base = self.positions.len() as u32;
        let n = normal.normalize_or_zero().to_array();
        for (p, uv) in corners.iter().zip(uvs) {
            self.positions.push(p.to_array());
            self.normals.push(n);
            self.uvs.push(uv);
        }
        self.indices.extend([base, base + 2, base + 1, base, base + 3, base + 2]);
    }

    /// Append a Bevy primitive mesh placed by `at`.
    fn mesh(&mut self, mesh: &Mesh, at: Transform) {
        let base = self.positions.len() as u32;
        let affine = at.compute_affine();
        let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            return;
        };
        let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(n)) => n.clone(),
            _ => vec![[0.0, 1.0, 0.0]; positions.len()],
        };
        let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(uv)) => uv.clone(),
            _ => vec![[0.0, 0.0]; positions.len()],
        };
        for ((p, n), uv) in positions.iter().zip(normals).zip(uvs) {
            self.positions.push(affine.transform_point3(Vec3::from_array(*p)).to_array());
            self.normals.push(affine.transform_vector3(Vec3::from_array(n)).normalize_or_zero().to_array());
            self.uvs.push(uv);
        }
        match mesh.indices() {
            Some(Indices::U16(i)) => self.indices.extend(i.iter().map(|&i| base + i as u32)),
            Some(Indices::U32(i)) => self.indices.extend(i.iter().map(|&i| base + i)),
            None => self.indices.extend((0..positions.len() as u32).map(|i| base + i)),
        }
    }

    fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[derive(Component)]
pub(crate) struct ParkRoot;

#[derive(Resource, Default)]
pub(super) struct ParkView {
    /// (map, barrier) last built.
    built: Option<(usize, bool)>,
    root: Option<Entity>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_park(
    mut commands: Commands,
    mut view: ResMut<ParkView>,
    placed: Res<PlacedPieces>,
    rig: Res<Rig>,
    textures: Option<Res<MarcoTextures>>,
    server: Res<AssetServer>,
    manifest: Res<crate::assets::AssetManifest>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut roots: Query<&mut Transform, With<ParkRoot>>,
) {
    let wanted = placed.park.map(|p| (p.map, p.fence));
    if view.built != wanted {
        if let Some(root) = view.root.take() {
            commands.entity(root).despawn();
        }
        view.built = wanted;
        if let (Some((map, fence)), Some(textures)) = (wanted, textures.as_deref()) {
            let statue = server.load(GltfAssetLabel::Scene(0).from_asset(manifest.0.character_scene.clone()));
            view.root = Some(build(&mut commands, map, fence, textures, statue, &mut meshes, &mut materials));
        }
    }
    let (Some(root), Some(park)) = (view.root, placed.park) else { return };
    let (Ok(mut transform), Some(target)) = (roots.get_mut(root), park.world_transform(&rig)) else {
        return;
    };
    if *transform != target {
        *transform = target;
    }
}

fn build(
    commands: &mut Commands,
    map: usize,
    fence: bool,
    textures: &MarcoTextures,
    statue: Handle<Scene>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let mut buffers: HashMap<Paint, Buffer> = HashMap::default();
    let mut figures = Vec::new();
    let mut statue_spot = None;
    let tube = Mesh::from(Cylinder::new(0.055, 1.0).mesh().resolution(8));
    for (part, at, turn) in local_parts(map, fence) {
        let place = Transform::from_translation(at).with_rotation(Quat::from_rotation_y(turn));
        let slab = part == Part::Piece(PieceKind::Slab, 1.0);
        for (surface, geometry) in part.shapes() {
            // Rail tubes are drawn round (below), not as their square collision.
            for q in &geometry.quads {
                let n = quad_normal(q).normalize_or_zero();
                let top = q.iter().map(|p| p.y).fold(f32::MIN, f32::max);
                let paint = match surface {
                    // Riding surface orange, flat decks a darker orange, the
                    // side and back walls teal, as in the redesign.
                    Surface::Wood if n.y.abs() < 0.3 => Paint::RampSide,
                    Surface::Wood if n.y > 0.98 && top > 0.3 => Paint::Deck,
                    Surface::Concrete if slab && n.y < 0.5 => Paint::Tray,
                    s => Paint::Surface(s),
                };
                let a = n.abs();
                let uv = |p: Vec3| {
                    let p = p / TILE;
                    if a.y >= a.x.max(a.z) {
                        [p.x, p.z]
                    } else if a.x >= a.z {
                        [p.z, -p.y]
                    } else {
                        [p.x, -p.y]
                    }
                };
                let corners = q.map(|p| place.transform_point(p));
                buffers.entry(paint).or_default().quad(corners, q.map(uv), place.rotation * n);
            }
            for d in &geometry.decals {
                let right = Vec3::Y.cross(d.normal).normalize_or_zero() * d.height * STICKER_ASPECT * 0.5;
                let up = Vec3::Y * d.height * 0.5;
                let c = d.centre;
                let corners = [c - right - up, c - right + up, c + right + up, c + right - up].map(|p| place.transform_point(p));
                buffers.entry(Paint::Sticker).or_default().quad(
                    corners,
                    [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
                    place.rotation * d.normal,
                );
            }
            for rail in &geometry.rails {
                for pair in rail.windows(2) {
                    let (a, b) = (place.transform_point(pair[0]), place.transform_point(pair[1]));
                    let length = a.distance(b);
                    if length < 1e-3 {
                        continue;
                    }
                    let t = Transform::from_translation((a + b) * 0.5)
                        .with_rotation(Quat::from_rotation_arc(Vec3::Y, (b - a) / length))
                        .with_scale(Vec3::new(1.0, length, 1.0));
                    buffers.entry(Paint::Coping).or_default().mesh(&tube, t);
                }
            }
        }
        if let Part::Piece(PieceKind::Statue, size) = part {
            statue_spot = Some((at, size));
            figures.push(
                Transform::from_translation(at + Vec3::Y * PEDESTAL_HEIGHT * size)
                    .with_rotation(Quat::from_rotation_y(turn))
                    .with_scale(Vec3::splat(STATUE_FIGURE_SCALE * size)),
            );
        }
    }
    scenery(&mut buffers, map, statue_spot);

    let blue_glow = |intensity: f32| StandardMaterial {
        base_color: MARCO_BLUE,
        emissive: LinearRgba::from(MARCO_BLUE) * intensity,
        ..default()
    };
    let plain = |color: Color, roughness: f32| StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        ..default()
    };
    let textured = |texture: &Handle<Image>, color: Color, roughness: f32, metallic: f32| StandardMaterial {
        base_color: color,
        base_color_texture: Some(texture.clone()),
        perceptual_roughness: roughness,
        metallic,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    let root = commands
        .spawn((ParkRoot, Transform::default(), Visibility::default()))
        .id();
    for (paint, buffer) in buffers {
        let material = match paint {
            Paint::Surface(Surface::Wood) => textured(&textures.plywood, Color::WHITE, 0.48, 0.08),
            Paint::Deck => textured(&textures.plywood, Color::srgb(0.88, 0.85, 0.82), 0.58, 0.08),
            Paint::RampSide => textured(&textures.concrete, Color::srgb(0.23, 0.45, 0.43), 0.68, 0.1),
            Paint::Surface(Surface::Concrete) => textured(&textures.concrete, Color::WHITE, 0.82, 0.04),
            Paint::Surface(Surface::Stairs) => plain(Color::srgb(0.97, 0.98, 0.99), 0.55),
            Paint::Surface(Surface::Granite) => textured(&textures.concrete, Color::srgb(0.24, 0.26, 0.29), 0.42, 0.22),
            Paint::Surface(Surface::Metal) | Paint::Coping => StandardMaterial {
                base_color: Color::srgb(0.95, 0.96, 0.98),
                metallic: 0.92,
                perceptual_roughness: 0.18,
                ..default()
            },
            Paint::Tray => StandardMaterial {
                base_color: Color::srgb(0.58, 0.64, 0.72),
                metallic: 0.55,
                perceptual_roughness: 0.4,
                ..default()
            },
            Paint::Surface(Surface::Marble) => textured(&textures.marble, Color::WHITE, 0.25, 0.0),
            Paint::Surface(Surface::Glass) => StandardMaterial {
                base_color: Color::srgba(0.58, 0.77, 0.99, 0.28),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.1,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
            Paint::Sticker => StandardMaterial {
                base_color_texture: Some(textures.sticker.clone()),
                alpha_mode: AlphaMode::Mask(0.5),
                perceptual_roughness: 0.6,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
            Paint::Leaves => plain(Color::srgb(0.29, 0.87, 0.5), 0.65),
            Paint::Bark => plain(Color::srgb(0.52, 0.3, 0.05), 0.9),
            Paint::Soil => plain(Color::srgb(0.18, 0.18, 0.2), 0.6),
            Paint::BenchWood => textured(&textures.plywood, Color::WHITE, 0.5, 0.05),
            Paint::DarkMetal => plain(Color::srgb(0.12, 0.13, 0.15), 0.4),
            Paint::Glow => StandardMaterial {
                base_color: Color::srgb(1.0, 0.9, 0.6),
                emissive: LinearRgba::rgb(4.0, 3.4, 2.0),
                ..default()
            },
            Paint::LightStrip => blue_glow(3.0),
            Paint::Ring => StandardMaterial {
                base_color: Color::srgb(0.38, 0.65, 0.98),
                emissive: LinearRgba::rgb(0.15, 0.3, 0.6),
                double_sided: true,
                cull_mode: None,
                ..default()
            },
        };
        commands.spawn((
            ChildOf(root),
            Mesh3d(meshes.add(buffer.into_mesh())),
            MeshMaterial3d(materials.add(material)),
            Transform::default(),
        ));
    }
    for at in figures {
        commands.spawn((ChildOf(root), StatueFigure, SceneRoot(statue.clone()), at));
    }
    root
}

/// The map's props (their collision is in `park::prop_collision`), the base's
/// light strip and the ring round the statue, park frame.
fn scenery(buffers: &mut HashMap<Paint, Buffer>, map: usize, statue: Option<(Vec3, f32)>) {
    let deck = SLAB_SIZE.y;
    let (hx, hz) = (SLAB_SIZE.x * 0.5, SLAB_SIZE.z * 0.5);
    fn put(buffers: &mut HashMap<Paint, Buffer>, paint: Paint, mesh: impl Into<Mesh>, at: Transform) {
        buffers.entry(paint).or_default().mesh(&mesh.into(), at);
    }
    for (prop, base, turn) in local_props(map) {
        let frame = Transform::from_translation(base).with_rotation(Quat::from_rotation_y(turn));
        let at = |x: f32, y: f32, z: f32| frame.mul_transform(Transform::from_xyz(x, y, z));
        match prop {
            Prop::Planter => {
                put(buffers, Paint::Surface(Surface::Granite), Cuboid::new(2.2, 0.7, 2.2), at(0.0, 0.35, 0.0));
                put(buffers, Paint::Soil, Cuboid::new(1.9, 0.05, 1.9), at(0.0, 0.72, 0.0));
                put(buffers, Paint::Bark, Cylinder::new(0.14, 2.0).mesh().resolution(6), at(0.0, 1.7, 0.0));
                for (dx, dy, dz, r) in [(0.0, 3.0, 0.0, 1.1), (0.45, 2.6, 0.3, 0.8), (-0.4, 2.7, -0.3, 0.8), (0.0, 3.7, 0.1, 0.7)] {
                    put(buffers, Paint::Leaves, Sphere::new(r).mesh().ico(1).unwrap(), at(dx, dy, dz));
                }
            }
            Prop::Bench => {
                put(buffers, Paint::BenchWood, Cuboid::new(2.4, 0.12, 0.6), at(0.0, 0.5, 0.0));
                put(buffers, Paint::BenchWood, Cuboid::new(2.4, 0.5, 0.1), at(0.0, 0.85, -0.3));
                for dx in [-1.0, 1.0] {
                    put(buffers, Paint::DarkMetal, Cuboid::new(0.1, 0.5, 0.55), at(dx, 0.25, 0.0));
                }
            }
            Prop::Lamp => {
                put(buffers, Paint::DarkMetal, Cylinder::new(0.08, 4.0).mesh().resolution(6), at(0.0, 2.0, 0.0));
                put(buffers, Paint::DarkMetal, Cuboid::new(0.7, 0.15, 0.4), at(0.0, 4.05, 0.0));
                put(buffers, Paint::Glow, Cuboid::new(0.55, 0.06, 0.3), at(0.0, 3.95, 0.0));
            }
            Prop::Flag => {
                put(buffers, Paint::DarkMetal, Cylinder::new(0.06, 6.0).mesh().resolution(6), at(0.0, 3.0, 0.0));
                // The banner flies towards the deck's middle.
                let side = if base.x < 0.0 { 1.0 } else { -1.0 };
                let w = 2.4;
                let h = w / STICKER_ASPECT;
                let c = base + Vec3::new(side * (w * 0.5 + 0.06), 5.2, 0.0);
                let (r, u) = (Vec3::X * w * 0.5, Vec3::Y * h * 0.5);
                buffers.entry(Paint::Sticker).or_default().quad(
                    [c - r - u, c - r + u, c + r + u, c + r - u],
                    [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
                    Vec3::Z,
                );
            }
        }
    }
    // A blue light strip round the base, just under the deck's edge.
    let strip = buffers.entry(Paint::LightStrip).or_default();
    for (size, x, z) in [
        (Vec3::new(SLAB_SIZE.x + 0.1, 0.12, 0.05), 0.0, hz + 0.03),
        (Vec3::new(SLAB_SIZE.x + 0.1, 0.12, 0.05), 0.0, -hz - 0.03),
        (Vec3::new(0.05, 0.12, SLAB_SIZE.z + 0.1), hx + 0.03, 0.0),
        (Vec3::new(0.05, 0.12, SLAB_SIZE.z + 0.1), -hx - 0.03, 0.0),
    ] {
        strip.mesh(&Mesh::from(Cuboid::from_size(size)), Transform::from_xyz(x, deck - 0.2, z));
    }
    // A painted ring on the deck round the statue.
    let Some((centre, size)) = statue else { return };
    let ring = buffers.entry(Paint::Ring).or_default();
    let inner = 1.2 * size + 1.0;
    let (outer, centre) = (inner + 0.4, centre + Vec3::Y * 0.012);
    let steps = 40;
    for i in 0..steps {
        let (a0, a1) = (TAU * i as f32 / steps as f32, TAU * (i + 1) as f32 / steps as f32);
        let p = |r: f32, a: f32| centre + Vec3::new(r * a.cos(), 0.0, r * a.sin());
        ring.quad([p(inner, a0), p(outer, a0), p(outer, a1), p(inner, a1)], [[0.0; 2]; 4], Vec3::Y);
    }
}
