//! Eye-view quality that follows the world scale, and a frame-time log per scale.
//!
//! The scale only resizes the tracking root (see [`rig`]): a big skater draws
//! the same scene, but the park fills the whole view, so every eye pixel pays
//! for MSAA. Shadows are already off in XR, so MSAA is the knob left: 4x for a
//! tabletop miniature, 2x at mid size, off near life size. Each change rebuilds
//! the eye targets and pipelines (a hitch), so the tiers have a margin and wait
//! for the stick to be let go.
//!
//! Every [`LOG_EVERY_S`] the log gets one `XR PERF` line with the scale, the MSAA
//! in use, the fps and the average and worst frame time, to see on the Quest
//! where it gets heavy.
//!
//! [`rig`]: super::rig
use super::build::PlacedPieces;
use super::rig::Rig;
use bevy::prelude::*;
use bevy_mod_xr::camera::XrCamera;

/// At this scale and above (game metres per real metre) the eyes get 4x MSAA.
const FULL_MSAA_FROM: f32 = 6.0;
/// At this scale and above, 2x; below it, no MSAA.
const HALF_MSAA_FROM: f32 = 2.5;
/// How far past a tier bound the scale must go to switch back, as a fraction.
const MARGIN: f32 = 0.1;
/// Seconds between `XR PERF` log lines.
const LOG_EVERY_S: f32 = 2.0;

pub(crate) struct QualityPlugin;

impl Plugin for QualityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (follow_scale, log_frames));
    }
}

/// MSAA for `scale`, keeping `current` while the scale is within [`MARGIN`]
/// of the bound it would cross.
fn tier(scale: f32, current: Msaa) -> Msaa {
    let pick = |full: f32, half: f32| {
        if scale >= full {
            Msaa::Sample4
        } else if scale >= half {
            Msaa::Sample2
        } else {
            Msaa::Off
        }
    };
    let wanted = pick(FULL_MSAA_FROM, HALF_MSAA_FROM);
    // Moving to a cheaper tier needs the scale clearly below the bound,
    // moving back needs it clearly above.
    let (full, half) = match current {
        Msaa::Sample4 => (FULL_MSAA_FROM * (1.0 - MARGIN), HALF_MSAA_FROM * (1.0 - MARGIN)),
        Msaa::Off => (FULL_MSAA_FROM * (1.0 + MARGIN), HALF_MSAA_FROM * (1.0 + MARGIN)),
        _ => (FULL_MSAA_FROM * (1.0 + MARGIN), HALF_MSAA_FROM * (1.0 - MARGIN)),
    };
    if wanted == current { current } else { pick(full, half) }
}

fn follow_scale(rig: Res<Rig>, placed: Res<PlacedPieces>, mut eyes: Query<&mut Msaa, With<XrCamera>>) {
    if placed.rescaling {
        return;
    }
    for mut msaa in &mut eyes {
        let next = tier(rig.scale, *msaa);
        if *msaa != next {
            info!("XR QUALITY world scale {:.2}: eye MSAA {:?} -> {:?}", rig.scale, *msaa, next);
            *msaa = next;
        }
    }
}

#[derive(Default)]
struct Window {
    elapsed: f32,
    frames: u32,
    worst: f32,
}

fn log_frames(
    time: Res<Time<Real>>,
    rig: Res<Rig>,
    eyes: Query<&Msaa, With<XrCamera>>,
    mut window: Local<Window>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    window.elapsed += dt;
    window.frames += 1;
    window.worst = window.worst.max(dt);
    if window.elapsed < LOG_EVERY_S {
        return;
    }
    let msaa = eyes.iter().next().copied();
    info!(
        "XR PERF world scale {:.2} msaa {:?}: {:.1} fps, frame {:.1} ms avg, {:.1} ms worst",
        rig.scale,
        msaa,
        window.frames as f32 / window.elapsed,
        window.elapsed * 1000.0 / window.frames as f32,
        window.worst * 1000.0,
    );
    *window = Window::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_follow_scale() {
        assert_eq!(tier(32.0, Msaa::Sample4), Msaa::Sample4);
        assert_eq!(tier(4.0, Msaa::Sample4), Msaa::Sample2);
        assert_eq!(tier(1.0, Msaa::Sample4), Msaa::Off);
        assert_eq!(tier(1.0, Msaa::Off), Msaa::Off);
        assert_eq!(tier(32.0, Msaa::Off), Msaa::Sample4);
    }

    #[test]
    fn margin_keeps_the_tier_near_a_bound() {
        // Just under 6 stays 4x, just over 6 does not leave 2x.
        assert_eq!(tier(5.8, Msaa::Sample4), Msaa::Sample4);
        assert_eq!(tier(6.2, Msaa::Sample2), Msaa::Sample2);
        assert_eq!(tier(6.7, Msaa::Sample2), Msaa::Sample4);
        // Same around 2.5.
        assert_eq!(tier(2.4, Msaa::Sample2), Msaa::Sample2);
        assert_eq!(tier(2.6, Msaa::Off), Msaa::Off);
        assert_eq!(tier(2.2, Msaa::Sample2), Msaa::Off);
    }
}
