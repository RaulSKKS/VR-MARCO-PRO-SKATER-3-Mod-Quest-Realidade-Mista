//! The Marco park's own obstacles, modelled after the "Skate VR Tabletop
//! Architect" redesign: every ramp has a real deck behind it, every box can
//! be ridden onto from its ends, stairs come down from a raised deck. They
//! are only used by the park ([`super::park`]), so they take their real
//! dimensions (metres) instead of a single size.
//!
//! Each part is built in its own frame (+Y up, base on y = 0, a quarter pipe
//! or bank opening towards +Z, long things along X) as one or more pieces of
//! geometry, each with the surface it is drawn with.
use super::pieces::{
    PieceGeometry, PieceKind, Piece, Surface, box_quads, coping, extrude, grind_box, push, tube_quads,
};
use bevy::prelude::*;
use std::f32::consts::FRAC_PI_2;

/// Rail and coping tubes, game metres.
const TUBE: f32 = 0.08;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Part {
    /// A stock placed piece (`pieces`): kind and size.
    Piece(PieceKind, f32),
    /// Quarter pipe whose curve meets the floor tangentially, with a flat
    /// deck behind it and coping on the lip.
    Quarter { width: f32, height: f32, run: f32, deck: f32 },
    /// A straight bank up to a deck.
    Bank { width: f32, height: f32, run: f32, deck: f32 },
    /// Up, across a flat table, down (along X). With a gap: two kickers with
    /// a grind box between them.
    Funbox { length: f32, width: f32, height: f32, top: f32, hubba: bool, rail: bool, gap: bool },
    /// A raised concrete deck.
    DeckBox { width: f32, height: f32, depth: f32 },
    /// Stairs down from a deck at -Z to the floor at +Z, with hubba ledges
    /// on both sides and a handrail down the middle.
    Stairs { width: f32, height: f32, run: f32, steps: u32 },
    /// A granite box with bevelled ends (along X).
    Ledge { length: f32, height: f32, width: f32 },
    /// A flat bar (along X).
    FlatRail { length: f32, height: f32 },
    /// A four-sided hip pyramid with a flat top and two ledges on it.
    Pyramid { base_w: f32, base_d: f32, top_w: f32, top_d: f32, height: f32 },
}

/// Move every point of `g` by `f`.
pub(crate) fn map_geometry(g: &mut PieceGeometry, f: impl Fn(Vec3) -> Vec3, turn: Quat) {
    for q in g.quads.iter_mut().chain(g.rail_quads.iter_mut()) {
        *q = q.map(&f);
    }
    for r in &mut g.rails {
        for p in r.iter_mut() {
            *p = f(*p);
        }
    }
    for e in &mut g.edges {
        *e = (f(e.0), f(e.1));
    }
    for d in &mut g.decals {
        d.centre = f(d.centre);
        d.normal = turn * d.normal;
    }
}

/// `g` turned about +Y and moved to `at`.
pub(crate) fn placed(mut g: PieceGeometry, at: Vec3, turn: f32) -> PieceGeometry {
    let r = Quat::from_rotation_y(turn);
    map_geometry(&mut g, |p| at + r * p, r);
    g
}

/// Length-along-Z shapes turned so their length runs along X.
fn along_x(g: PieceGeometry) -> PieceGeometry {
    placed(g, Vec3::ZERO, FRAC_PI_2)
}

fn shifted_x(mut g: PieceGeometry, dx: f32) -> PieceGeometry {
    map_geometry(&mut g, |p| p + Vec3::X * dx, Quat::IDENTITY);
    g
}

/// A round-ish grind bar along `points` (in a y-z plane) with posts down
/// to `floor` heights.
fn bar(points: &[Vec3], posts: &[(Vec3, f32)]) -> (PieceGeometry, PieceGeometry) {
    let mut rail = PieceGeometry::default();
    for w in points.windows(2) {
        tube_quads(&mut rail.rail_quads, w[0], w[1], TUBE);
    }
    rail.rails.push(points.to_vec());
    let mut legs = PieceGeometry::default();
    for &(top, floor) in posts {
        box_quads(&mut legs.quads, Vec3::new(top.x - 0.04, floor, top.z - 0.04), Vec3::new(top.x + 0.04, top.y - TUBE, top.z + 0.04));
    }
    (rail, legs)
}

impl Part {
    /// The part's geometry, each piece with its surface.
    pub(crate) fn shapes(&self) -> Vec<(Surface, PieceGeometry)> {
        match *self {
            Part::Piece(kind, size) => vec![(kind.surface(), Piece { kind, size }.geometry())],
            Part::Quarter { width, height, run, deck } => {
                let (front, back) = ((run + deck) * 0.5, -(run + deck) * 0.5);
                let lip = front - run;
                let mut profile = vec![Vec2::new(back, height), Vec2::new(lip, height)];
                profile.extend((1..=16).map(|i| {
                    let s = 1.0 - i as f32 / 16.0;
                    let a = s * FRAC_PI_2;
                    Vec2::new(front - run * a.sin(), height * (1.0 - a.cos()))
                }));
                let mut g = PieceGeometry::default();
                extrude(&mut g, &profile, width);
                coping(&mut g, lip, height, width);
                g.edges.push((Vec3::new(-width * 0.5, height, back), Vec3::new(width * 0.5, height, back)));
                vec![(Surface::Wood, g)]
            }
            Part::Bank { width, height, run, deck } => {
                let (front, back) = ((run + deck) * 0.5, -(run + deck) * 0.5);
                let lip = front - run;
                let mut g = PieceGeometry::default();
                extrude(&mut g, &[Vec2::new(back, height), Vec2::new(lip, height), Vec2::new(front, 0.0)], width);
                coping(&mut g, lip, height, width);
                vec![(Surface::Wood, g)]
            }
            Part::Funbox { length, width, height, top, hubba, rail, gap } => {
                let (l, t) = (length * 0.5, top * 0.5);
                let mut out = Vec::new();
                let mut body = PieceGeometry::default();
                if gap {
                    extrude(&mut body, &[Vec2::new(-l, 0.0), Vec2::new(-t, height)], width);
                    extrude(&mut body, &[Vec2::new(t, height), Vec2::new(l, 0.0)], width);
                    let mut gap_box = PieceGeometry::default();
                    grind_box(&mut gap_box, Vec3::new(0.85, height * 0.75, top * 0.75), [false, true, false, true]);
                    gap_box.decals.clear();
                    out.push((Surface::Granite, along_x(gap_box)));
                } else {
                    extrude(&mut body, &[Vec2::new(-l, 0.0), Vec2::new(-t, height), Vec2::new(t, height), Vec2::new(l, 0.0)], width);
                    coping(&mut body, -t, height, width);
                    coping(&mut body, t, height, width);
                }
                out.push((Surface::Wood, along_x(body)));
                if hubba {
                    let (hh, hw) = (0.38, 0.55);
                    let mut h = PieceGeometry::default();
                    let profile = [
                        Vec2::new(-l + 0.2, hh),
                        Vec2::new(-t, height + hh),
                        Vec2::new(t, height + hh),
                        Vec2::new(l - 0.2, hh),
                    ];
                    extrude(&mut h, &profile, hw);
                    h.decals.clear();
                    for side in [-hw * 0.5, hw * 0.5] {
                        h.rails.push(profile.iter().map(|p| Vec3::new(side, p.y, p.x)).collect());
                    }
                    out.push((Surface::Granite, along_x(shifted_x(h, -width * 0.5 + hw * 0.5 + 0.05))));
                }
                if rail {
                    let (lift, x) = (0.52, width * 0.5 - 0.35);
                    let points = [
                        Vec3::new(x, lift * 0.85, -l + 0.4),
                        Vec3::new(x, height + lift, -t),
                        Vec3::new(x, height + lift, t),
                        Vec3::new(x, lift * 0.85, l - 0.4),
                    ];
                    let floor_at = |z: f32| if gap && z.abs() < t { 0.0 } else if z.abs() <= t { height } else { height * (l - z.abs()) / (l - t) };
                    let posts: Vec<(Vec3, f32)> = [-l + 0.7, -t, t, l - 0.7]
                        .iter()
                        .map(|&z| {
                            let top_y = if z.abs() <= t + 1e-3 { height + lift } else { lift * 0.85 + (height + lift - lift * 0.85) * (l - 0.4 - z.abs()).max(0.0) / (l - 0.4 - t) };
                            (Vec3::new(x, top_y, z), floor_at(z))
                        })
                        .collect();
                    let (bar_g, legs) = bar(&points, &posts);
                    out.push((Surface::Metal, along_x(bar_g)));
                    out.push((Surface::Granite, along_x(legs)));
                }
                out
            }
            Part::DeckBox { width, height, depth } => {
                let mut g = PieceGeometry::default();
                grind_box(&mut g, Vec3::new(width, height, depth), [true; 4]);
                vec![(Surface::Concrete, g)]
            }
            Part::Stairs { width, height, run, steps } => {
                let hubba_w = 0.52;
                let core = width - hubba_w * 2.0;
                let (rise, tread) = (height / steps as f32, run / steps as f32);
                let z0 = -run * 0.5;
                let mut profile = vec![Vec2::new(z0, height)];
                for i in 1..=steps {
                    let z = z0 + tread * i as f32;
                    profile.push(Vec2::new(z - tread, height - rise * (i - 1) as f32));
                    profile.push(Vec2::new(z - tread, height - rise * i as f32));
                    profile.push(Vec2::new(z, height - rise * i as f32));
                }
                profile.dedup();
                let mut steps_g = PieceGeometry::default();
                extrude(&mut steps_g, &profile, core);
                steps_g.decals.clear();
                let mut out = vec![(Surface::Stairs, steps_g)];
                let extra = 0.28;
                for side in [-1.0, 1.0] {
                    let mut h = PieceGeometry::default();
                    let profile = [Vec2::new(z0, height + extra), Vec2::new(-z0, extra)];
                    extrude(&mut h, &profile, hubba_w);
                    h.decals.clear();
                    h.rails.push(profile.iter().map(|p| Vec3::new(0.0, p.y, p.x)).collect());
                    out.push((Surface::Granite, shifted_x(h, side * (width * 0.5 - hubba_w * 0.5))));
                }
                let lift = 0.58;
                let (a, b) = (Vec3::new(0.0, height + lift, z0 + 0.15), Vec3::new(0.0, lift * 0.85, -z0 - 0.1));
                let (rail, legs) = bar(&[a, b], &[(Vec3::new(0.0, height + lift, z0 + 0.2), height), (Vec3::new(0.0, lift * 0.85, -z0 - 0.2), 0.0)]);
                out.push((Surface::Metal, rail));
                out.push((Surface::Granite, legs));
                out
            }
            Part::Ledge { length, height, width } => {
                let bevel = (length * 0.22).min(0.85);
                let (l, f) = (length * 0.5, length * 0.5 - bevel);
                let mut g = PieceGeometry::default();
                extrude(&mut g, &[Vec2::new(-l, 0.0), Vec2::new(-f, height), Vec2::new(f, height), Vec2::new(l, 0.0)], width);
                g.decals.clear();
                for side in [-width * 0.5, width * 0.5] {
                    let edge = (Vec3::new(side, height, -f), Vec3::new(side, height, f));
                    g.rails.push(vec![edge.0, edge.1]);
                    g.edges.push(edge);
                }
                vec![(Surface::Granite, along_x(g))]
            }
            Part::FlatRail { length, height } => {
                let l = length * 0.5;
                let (rail, legs) = bar(
                    &[Vec3::new(0.0, height, -l), Vec3::new(0.0, height, l)],
                    &[(Vec3::new(0.0, height, -length * 0.38), 0.0), (Vec3::new(0.0, height, length * 0.38), 0.0)],
                );
                vec![(Surface::Metal, along_x(rail)), (Surface::Granite, along_x(legs))]
            }
            Part::Pyramid { base_w, base_d, top_w, top_d, height } => {
                let (bw, bd, tw, td) = (base_w * 0.5, base_d * 0.5, top_w * 0.5, top_d * 0.5);
                let b = |x: f32, z: f32| Vec3::new(x, 0.0, z);
                let t = |x: f32, z: f32| Vec3::new(x, height, z);
                let mut slopes = PieceGeometry::default();
                push(&mut slopes.quads, [b(-bw, -bd), b(bw, -bd), t(tw, -td), t(-tw, -td)], Vec3::new(0.0, 1.0, -1.0));
                push(&mut slopes.quads, [b(bw, bd), b(-bw, bd), t(-tw, td), t(tw, td)], Vec3::new(0.0, 1.0, 1.0));
                push(&mut slopes.quads, [b(-bw, bd), b(-bw, -bd), t(-tw, -td), t(-tw, td)], Vec3::new(-1.0, 1.0, 0.0));
                push(&mut slopes.quads, [b(bw, -bd), b(bw, bd), t(tw, td), t(tw, -td)], Vec3::new(1.0, 1.0, 0.0));
                let mut top = PieceGeometry::default();
                push(&mut top.quads, [t(-tw, -td), t(tw, -td), t(tw, td), t(-tw, td)], Vec3::Y);
                let mut ledges = PieceGeometry::default();
                for side in [-1.0, 1.0] {
                    let z = side * (td - 0.4);
                    let (lo, hi) = (Vec3::new(-tw * 0.85, height, z - 0.325), Vec3::new(tw * 0.85, height + 0.42, z + 0.325));
                    box_quads(&mut ledges.quads, lo, hi);
                    for edge_z in [lo.z, hi.z] {
                        let edge = (Vec3::new(lo.x, hi.y, edge_z), Vec3::new(hi.x, hi.y, edge_z));
                        ledges.rails.push(vec![edge.0, edge.1]);
                        ledges.edges.push(edge);
                    }
                }
                vec![(Surface::Wood, slopes), (Surface::Concrete, top), (Surface::Granite, ledges)]
            }
        }
    }

    /// Which way a quarter pipe or bank opens (its own +Z), for checks.
    pub(crate) fn opens(&self) -> bool {
        matches!(self, Part::Quarter { .. } | Part::Bank { .. } | Part::Piece(PieceKind::QuarterPipe | PieceKind::Bank, _))
    }
}
