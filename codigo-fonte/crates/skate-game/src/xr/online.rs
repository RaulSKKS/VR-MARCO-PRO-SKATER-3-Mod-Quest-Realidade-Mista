//! The menu's online page: make a room (its code shows big, to read out to friends), join one by choosing its six
//! letters with the stick, leave it. Text is built from the glyph atlas in `marcovr`, like the score board's, so
//! it is ASCII only (no accents).
use super::marcovr::MarcoTextures;
use super::menu::{Page, XrMenu};
use super::rig::Rig;
use crate::multiplayer::{Multiplayer, RemoteActor};
use bevy::prelude::*;

/// The letters a room code is made of (the matchmaking Worker's: no I, L, O, 0, 1).
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
/// The letters a player name is made of (a space first: a name shorter than its slots ends in spaces).
const NAME_LETTERS: &[u8] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const NAME_SLOTS: usize = 12;
const ROWS: usize = 5;
const PANEL_W_M: f32 = 0.6;
const PANEL_H_M: f32 = 0.56;
const LINES: usize = 12;
const BLUE: Color = Color::srgb(0.494, 0.784, 0.949);
const YELLOW: Color = Color::srgb(1.0, 0.83, 0.0);
/// Name tags over remote skaters: text height, and how far above the skater's root (game metres).
const TAG_HEIGHT: f32 = 0.45;
const TAG_ABOVE: f32 = 2.3;

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Name,
    Code,
}

pub(super) struct Online {
    row: usize,
    players: u32,
    code: [usize; 6],
    name: [usize; NAME_SLOTS],
    cursor: usize,
    editing: Option<Field>,
}
impl Default for Online {
    fn default() -> Self {
        Self {
            row: 1,
            players: 4,
            code: [0; 6],
            name: [0; NAME_SLOTS],
            cursor: 0,
            editing: None,
        }
    }
}

/// Letters picked from `letters`, shown with the one at `cursor` in brackets while it is being edited.
fn spelled(picked: &[usize], letters: &[u8], cursor: Option<usize>) -> String {
    picked
        .iter()
        .enumerate()
        .map(|(i, &l)| {
            let c = match letters[l] {
                b' ' if cursor.is_some() => '_',
                c => c as char,
            };
            if Some(i) == cursor { format!("[{c}]") } else if cursor.is_some() { format!(" {c} ") } else { c.to_string() }
        })
        .collect()
}

impl Online {
    fn code(&self) -> String {
        spelled(&self.code, ALPHABET, None)
    }
    fn typed_name(&self) -> String {
        spelled(&self.name, NAME_LETTERS, None).trim().to_owned()
    }
    /// Starts editing the name from the one in use (letters it cannot show become spaces).
    fn edit_name(&mut self, current: &str) {
        let upper = current.to_ascii_uppercase();
        let mut letters = upper.bytes().map(|b| NAME_LETTERS.iter().position(|&l| l == b).unwrap_or(0));
        self.name = std::array::from_fn(|_| letters.next().unwrap_or(0));
        self.cursor = 0;
        self.editing = Some(Field::Name);
    }
    /// The controllers on this page; true when it is left for the main page.
    pub(super) fn input(
        &mut self,
        net: Option<&mut Multiplayer>,
        up_down: Option<i8>,
        sideways: Option<i8>,
        a: bool,
        b: bool,
    ) -> bool {
        if let Some(field) = self.editing {
            let (picked, letters): (&mut [usize], &[u8]) = match field {
                Field::Name => (&mut self.name, NAME_LETTERS),
                Field::Code => (&mut self.code, ALPHABET),
            };
            let last = picked.len() - 1;
            if let Some(d) = up_down {
                // stick up: the next letter
                let i = &mut picked[self.cursor];
                *i = (*i as i32 + d as i32).rem_euclid(letters.len() as i32) as usize;
            }
            if let Some(d) = sideways {
                self.cursor = (self.cursor as i32 + d as i32).clamp(0, last as i32) as usize;
            }
            if b {
                self.editing = None;
            } else if a && field == Field::Name {
                // the name is done at any letter (the rest stays blank)
                self.editing = None;
                if let Some(net) = net {
                    net.set_name(&self.typed_name());
                }
            } else if a && self.cursor < last {
                self.cursor += 1;
            } else if a {
                self.editing = None;
                let code = self.code();
                info!("MENU online join {code}");
                if let Some(net) = net {
                    net.room_join(&code);
                }
            }
            return false;
        }
        if b {
            return true;
        }
        if let Some(d) = up_down {
            self.row = (self.row as i32 - d as i32).rem_euclid(ROWS as i32) as usize;
        }
        if let (1, Some(d)) = (self.row, sideways) {
            self.players = (self.players as i32 + d as i32).clamp(2, 10) as u32;
        }
        if !a {
            return false;
        }
        match self.row {
            0 => self.edit_name(net.as_deref().map_or("", Multiplayer::name)),
            1 => {
                info!("MENU online host for {}", self.players);
                if let Some(net) = net {
                    net.room_host(self.players);
                }
            }
            2 => {
                self.editing = Some(Field::Code);
                self.cursor = 0;
            }
            3 => {
                if let Some(net) = net {
                    net.leave();
                }
            }
            _ => return true,
        }
        false
    }
    /// What the page says, line by line: text, height (m) and colour.
    fn lines(&self, net: Option<&Multiplayer>) -> Vec<(String, f32, Color)> {
        let mut out = vec![("ONLINE".to_owned(), 0.045, BLUE)];
        match net.and_then(Multiplayer::room_code) {
            Some(code) => out.push((format!("CODIGO DA SALA: {code}"), 0.04, YELLOW)),
            None => out.push((String::new(), 0.04, YELLOW)),
        }
        let status: String = net.map_or("", |n| n.status.as_str()).chars().take(52).collect();
        out.push((status, 0.022, Color::WHITE));
        // who is in the room
        let names = net.map_or_else(Vec::new, Multiplayer::names);
        let present = if names.len() > 1 {
            let list: Vec<String> = names.iter().map(|(_, n, me)| if *me { format!("{n} (voce)") } else { n.clone() }).collect();
            format!("Na sala: {}", list.join(", ")).chars().take(56).collect()
        } else {
            String::new()
        };
        out.push((present, 0.022, BLUE));
        out.push((String::new(), 0.012, Color::WHITE));
        let name = match self.editing {
            Some(Field::Name) => spelled(&self.name, NAME_LETTERS, Some(self.cursor)),
            _ => net.map_or("Skater", Multiplayer::name).to_owned(),
        };
        let code = spelled(&self.code, ALPHABET, (self.editing == Some(Field::Code)).then_some(self.cursor));
        let rows = [
            format!("Seu nome: {name}"),
            format!("Criar sala para {}   < >", self.players),
            format!("Entrar: {code}"),
            "Sair da sala".to_owned(),
            "Voltar".to_owned(),
        ];
        for (i, text) in rows.into_iter().enumerate() {
            let selected = i == self.row;
            let mark = if selected { "> " } else { "  " };
            out.push((format!("{mark}{text}"), 0.03, if selected { YELLOW } else { Color::WHITE }));
        }
        out.push((
            match self.editing {
                Some(Field::Name) => "Cima/baixo: letra | Lados: casa | A: pronto | B: cancelar",
                Some(Field::Code) => "Cima/baixo: letra | Lados: casa | A: proxima | B: cancelar",
                None => "Analogico: escolher | A: confirmar | B: voltar",
            }
            .to_owned(),
            0.02,
            YELLOW,
        ));
        out
    }
}

#[derive(Component)]
pub(super) enum OnlinePart {
    Panel,
    Line(usize),
}

pub(super) fn spawn_parts(
    mut commands: Commands,
    textures: Option<Res<MarcoTextures>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(textures) = textures else { return };
    commands.spawn((
        OnlinePart::Panel,
        Mesh3d(meshes.add(Rectangle::new(PANEL_W_M, PANEL_H_M))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.047, 0.055, 0.078, 0.94),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
    for i in 0..LINES {
        commands.spawn((
            OnlinePart::Line(i),
            Mesh3d(meshes.add(Rectangle::new(0.01, 0.01))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color_texture: Some(textures.font.clone()),
                alpha_mode: AlphaMode::Mask(0.4),
                unlit: true,
                ..default()
            })),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn show_parts(
    menu: Res<XrMenu>,
    rig: Res<Rig>,
    net: Option<Res<Multiplayer>>,
    textures: Option<Res<MarcoTextures>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut parts: Query<(&OnlinePart, &Mesh3d, &MeshMaterial3d<StandardMaterial>, &mut Transform, &mut Visibility)>,
    mut shown: Local<Vec<(String, f32)>>,
) {
    let Some(textures) = textures else { return };
    let open = menu.open && menu.page == Page::Online;
    let place = match (open, menu.pose, rig.root_transform()) {
        (true, Some(pose), Some(root)) => Some(root.mul_transform(pose)),
        _ => None,
    };
    let lines = menu.online.lines(net.as_deref());
    shown.resize(LINES, (String::new(), 0.0));
    let mut y = PANEL_H_M * 0.5 - 0.045;
    let mut at = [0.0; LINES];
    for (i, (_, height, _)) in lines.iter().enumerate().take(LINES) {
        at[i] = y - height * 0.5;
        y -= height + 0.016;
    }
    for (part, mesh, material, mut transform, mut visibility) in &mut parts {
        let local = match *part {
            OnlinePart::Panel => Some(Vec3::ZERO),
            OnlinePart::Line(i) => lines.get(i).map(|_| Vec3::new(0.0, at[i], 0.004)),
        };
        if let OnlinePart::Line(i) = *part
            && let (true, Some((text, height, color))) = (open, lines.get(i))
        {
            if shown[i] != (text.clone(), *height) {
                shown[i] = (text.clone(), *height);
                if let Some(target) = meshes.get_mut(&mesh.0) {
                    *target = textures.font_metrics.text_mesh(text, *height);
                }
                if let Some(m) = materials.get_mut(&material.0) {
                    m.base_color = *color;
                }
            }
        }
        let visible = match (place, local) {
            (Some(place), Some(local)) => {
                let target = place.mul_transform(Transform::from_translation(local));
                if *transform != target {
                    *transform = target;
                }
                Visibility::Visible
            }
            _ => Visibility::Hidden,
        };
        if *visibility != visible {
            *visibility = visible;
        }
    }
}

/// A name tag: which remote skater it floats over, and the name it shows.
#[derive(Component)]
pub(super) struct NameTag {
    actor: u64,
    shown: String,
}

/// Each remote skater gets their name floating over their head in Marco blue, turned towards the player; tags
/// come and go with the skaters.
#[allow(clippy::too_many_arguments)]
pub(super) fn name_tags(
    mut commands: Commands,
    net: Option<Res<Multiplayer>>,
    textures: Option<Res<MarcoTextures>>,
    skaters: Query<(&RemoteActor, &GlobalTransform)>,
    eyes: Query<&GlobalTransform, With<bevy_mod_xr::camera::XrCamera>>,
    mut tags: Query<(Entity, &mut NameTag, &Mesh3d, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(net), Some(textures)) = (net, textures) else { return };
    let eye = eyes.iter().next().map(GlobalTransform::translation);
    let mut tagged = vec![];
    for (entity, mut tag, mesh, mut transform) in &mut tags {
        let Some((_, at)) = skaters.iter().find(|(a, _)| a.0 == tag.actor) else {
            commands.entity(entity).despawn();
            continue;
        };
        tagged.push(tag.actor);
        let name = net.name_of(tag.actor).unwrap_or_default();
        if name != tag.shown {
            if let Some(target) = meshes.get_mut(&mesh.0) {
                *target = textures.font_metrics.text_mesh(&name, TAG_HEIGHT);
            }
            tag.shown = name;
        }
        let at = at.translation() + Vec3::Y * TAG_ABOVE;
        // the text faces +Z: point -Z away from the eyes, level
        let away = eye.map_or(Vec3::NEG_Z, |e| (at - e).with_y(0.0).normalize_or(Vec3::NEG_Z));
        *transform = Transform::from_translation(at).looking_to(away, Vec3::Y);
    }
    for (actor, at) in &skaters {
        if tagged.contains(&actor.0) {
            continue;
        }
        let name = net.name_of(actor.0).unwrap_or_default();
        commands.spawn((
            NameTag { actor: actor.0, shown: name.clone() },
            Mesh3d(meshes.add(textures.font_metrics.text_mesh(&name, TAG_HEIGHT))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: BLUE,
                base_color_texture: Some(textures.font.clone()),
                alpha_mode: AlphaMode::Mask(0.4),
                unlit: true,
                ..default()
            })),
            Transform::from_translation(at.translation() + Vec3::Y * TAG_ABOVE),
            Visibility::Visible,
        ));
    }
}
