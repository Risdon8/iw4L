//! Mod: headless movement harness. Runs the real `pm_move` with the real
//! `pmove_context` against a world made of infinite half-space planes, so
//! jump/land/strafe behaviour can be measured without launching the game.

use movement_iw4::{
    ANGLE2SHORT, CollisionBackend, FlatMantleAnimLength, GroundTraceInput, ZeroMantleRootDelta,
    pm_move,
};
use playerstate_iw4::{PlayerState, UserCmd, buttons};
use trace_iw4::{ENTITYNUM_WORLD, HITTYPE_ENTITY, Trace};

use super::pmove_context;
use crate::MovementTuning;
use crate::world::spawn_player_state;

const SURFACE_CLIP_EPSILON: f32 = 0.125;
const WALKABLE_NORMAL_Z: f32 = 0.7;
const TICK_MS: i32 = 17;

/// Solid on the side opposite `normal`: points with `normal·p < dist`.
#[derive(Clone, Copy)]
struct Plane {
    normal: [f32; 3],
    dist: f32,
}

struct PlaneWorld(Vec<Plane>);

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

impl CollisionBackend for PlaneWorld {
    fn trace(&self, input: GroundTraceInput) -> Trace {
        let mut best = Trace {
            fraction: 1.0,
            endpos: input.end,
            ..Trace::default()
        };
        for plane in &self.0 {
            let n = plane.normal;
            let support: f32 = (0..3)
                .map(|i| {
                    n[i] * if n[i] > 0.0 {
                        input.mins[i]
                    } else {
                        input.maxs[i]
                    }
                })
                .sum();
            let d1 = dot(n, input.start) + support - plane.dist;
            let d2 = dot(n, input.end) + support - plane.dist;
            if d1 <= 0.0 {
                best.startsolid = 1;
                if d2 <= 0.0 {
                    best.allsolid = 1;
                    best.fraction = 0.0;
                    best.endpos = input.start;
                    best.normal = n;
                    best.contents = 1;
                    return best;
                }
                continue;
            }
            if d2 >= SURFACE_CLIP_EPSILON || d2 >= d1 {
                continue;
            }
            let fraction = ((d1 - SURFACE_CLIP_EPSILON) / (d1 - d2)).max(0.0);
            if fraction < best.fraction {
                best.fraction = fraction;
                best.normal = n;
                best.contents = 1;
                best.hit_type = HITTYPE_ENTITY;
                best.hit_id = ENTITYNUM_WORLD;
                best.walkable = u8::from(n[2] >= WALKABLE_NORMAL_Z);
                best.endpos = [
                    input.start[0] + fraction * (input.end[0] - input.start[0]),
                    input.start[1] + fraction * (input.end[1] - input.start[1]),
                    input.start[2] + fraction * (input.end[2] - input.start[2]),
                ];
            }
        }
        best
    }
}

#[derive(Clone, Copy, Default)]
struct Intent {
    yaw: f32,
    forward: i8,
    right: i8,
    jump: bool,
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    speed: f32,
    z: f32,
    grounded: bool,
}

fn horizontal_speed(ps: &PlayerState) -> f32 {
    ps.velocity[0].hypot(ps.velocity[1])
}

fn run(
    world: &PlaneWorld,
    mut ps: PlayerState,
    tuning: MovementTuning,
    ticks: usize,
    mut intent: impl FnMut(usize, &PlayerState) -> Intent,
) -> (PlayerState, Vec<Sample>) {
    let mut old_buttons = 0;
    let mut samples = Vec::with_capacity(ticks);
    for tick in 0..ticks {
        let want = intent(tick, &ps);
        let buttons = if want.jump { buttons::JUMP } else { 0 };
        let mut cmd = UserCmd {
            server_time: ps.command_time + TICK_MS,
            buttons,
            angles: [0, (want.yaw * ANGLE2SHORT) as i32, 0],
            forwardmove: want.forward,
            rightmove: want.right,
            ..UserCmd::default()
        };
        let context = pmove_context(
            old_buttons,
            (1.0, 1.0, 1.0),
            false,
            false,
            1.0 / 200.0,
            1.0 / 200.0,
            true,
            false,
            0,
            0,
            0,
            false,
            tuning,
        );
        pm_move(
            &mut ps,
            &mut cmd,
            context,
            world,
            &FlatMantleAnimLength::default(),
            &ZeroMantleRootDelta,
        );
        old_buttons = buttons;
        samples.push(Sample {
            speed: horizontal_speed(&ps),
            z: ps.origin[2],
            grounded: ps.ground_entity_num != trace_iw4::ENTITYNUM_NONE as i32,
        });
    }
    (ps, samples)
}

fn flat_floor() -> PlaneWorld {
    PlaneWorld(vec![Plane {
        normal: [0.0, 0.0, 1.0],
        dist: 0.0,
    }])
}

fn standing_with_speed(speed: f32) -> PlayerState {
    let mut ps = spawn_player_state([0.0, 0.0, SURFACE_CLIP_EPSILON], [0.0, 0.0, 0.0]);
    ps.command_time = 1_000;
    ps.jump_time = -10_000;
    ps.velocity = [speed, 0.0, 0.0];
    ps
}

fn surf_on() -> MovementTuning {
    MovementTuning {
        surf: true,
        ..MovementTuning::default()
    }
}

/// Holding jump from a running start, no strafing: how much speed survives
/// three seconds of hopping?
fn held_jump(tuning: MovementTuning) -> Vec<Sample> {
    let (_, samples) = run(
        &flat_floor(),
        standing_with_speed(300.0),
        tuning,
        180,
        |_, _| Intent {
            jump: true,
            ..Intent::default()
        },
    );
    samples
}

/// Classic air strafe: hold right strafe and turn right at a steady rate
/// while holding jump.
fn strafe_hop(tuning: MovementTuning) -> Vec<Sample> {
    let (_, samples) = run(
        &flat_floor(),
        standing_with_speed(190.0),
        tuning,
        180,
        |tick, _| Intent {
            yaw: -(tick as f32) * 2.0,
            right: 127,
            jump: true,
            ..Intent::default()
        },
    );
    samples
}

fn last(samples: &[Sample]) -> Sample {
    *samples.last().expect("ran at least one tick")
}

/// Retail must stay retail: a landing cuts speed and a held jump does not
/// re-jump, so the player ends standing still.
#[test]
fn retail_held_jump_stops() {
    let samples = held_jump(MovementTuning::default());
    let end = last(&samples);
    assert!(end.grounded && end.speed < 1.0, "retail ended at {end:?}");
}

/// Surf: autobhop and no landing penalty, so 300 u/s survives every hop.
#[test]
fn surf_held_jump_keeps_speed() {
    let samples = held_jump(surf_on());
    let slowest = samples.iter().map(|s| s.speed).fold(f32::MAX, f32::min);
    assert!(slowest > 299.0, "surf dropped to {slowest}");
    assert!(
        samples.iter().filter(|s| !s.grounded).count() > samples.len() * 9 / 10,
        "surf spent too long on the ground"
    );
}

#[test]
fn strafe_hop_gains_speed_only_with_surf() {
    let retail = last(&strafe_hop(MovementTuning::default()));
    let surf = last(&strafe_hop(surf_on()));
    assert!(retail.speed < 200.0, "retail strafe hop reached {retail:?}");
    assert!(surf.speed > 320.0, "surf strafe hop only reached {surf:?}");
}

/// A 60° ramp (too steep to stand on) running along +x; solid on the +y side.
fn surf_ramp() -> PlaneWorld {
    let (sin, cos) = (60f32.to_radians().sin(), 60f32.to_radians().cos());
    PlaneWorld(vec![Plane {
        normal: [0.0, -sin, cos],
        dist: 0.0,
    }])
}

/// Riding the ramp along +x at 600 u/s, holding strafe into the ramp.
fn ride_ramp(tuning: MovementTuning) -> Vec<Sample> {
    let mut ps = spawn_player_state([0.0, -20.0, 0.0], [0.0, 0.0, 0.0]);
    ps.command_time = 1_000;
    ps.jump_time = -10_000;
    ps.ground_entity_num = trace_iw4::ENTITYNUM_NONE as i32;
    ps.velocity = [600.0, 0.0, 0.0];
    let (_, samples) = run(&surf_ramp(), ps, tuning, 120, |_, _| Intent {
        right: -127,
        ..Intent::default()
    });
    samples
}

/// Retail slides off a steep ramp; surf holds its line and keeps its speed.
#[test]
fn surf_rides_steep_ramp() {
    let retail = last(&ride_ramp(MovementTuning::default()));
    let surf = last(&ride_ramp(surf_on()));
    assert!(
        retail.z < -500.0,
        "retail should slide off the ramp: {retail:?}"
    );
    assert!(surf.z.abs() < 20.0, "surf should hold its line: {surf:?}");
    assert!(
        (surf.speed - 600.0).abs() < 5.0,
        "surf should keep its speed: {surf:?}"
    );
}

fn layout_backend(layout: &[crate::world::SimBrush]) -> super::ClipBackend<'_> {
    static EMPTY_BSP: std::sync::LazyLock<crate::world::SimClipBsp> =
        std::sync::LazyLock::new(crate::world::SimClipBsp::default);
    static EMPTY_MESH: std::sync::LazyLock<crate::world::SimClipMesh> =
        std::sync::LazyLock::new(crate::world::SimClipMesh::default);
    super::ClipBackend {
        brushes: &[],
        bsp: &EMPTY_BSP,
        mesh: &EMPTY_MESH,
        glass_damage: &[],
        bodies: &[],
        self_entnum: 0,
        cmodels: &[],
        linked_brushes: &[],
        layout,
    }
}

fn solid(planes: Vec<[f32; 4]>) -> crate::world::SimBrush {
    let n = planes.len();
    crate::world::SimBrush {
        planes,
        contents: 1,
        plane_surface_flags: vec![0; n],
        glass_encoded: 0,
    }
}

/// A 60° wedge along +x through the real capsule brush trace: base on z=0,
/// 256 wide, ridge on the x axis.
fn layout_wedge() -> crate::world::SimBrush {
    let (sin, cos) = (60f32.to_radians().sin(), 60f32.to_radians().cos());
    solid(vec![
        [0.0, 0.0, -1.0, 0.0],
        [1.0, 0.0, 0.0, 4096.0],
        [-1.0, 0.0, 0.0, 4096.0],
        [0.0, sin, cos, sin * 128.0],
        [0.0, -sin, cos, sin * 128.0],
    ])
}

fn run_backend(
    backend: &super::ClipBackend<'_>,
    mut ps: PlayerState,
    tuning: MovementTuning,
    ticks: usize,
    want: Intent,
) -> PlayerState {
    for _ in 0..ticks {
        let mut cmd = UserCmd {
            server_time: ps.command_time + TICK_MS,
            angles: [0, (want.yaw * ANGLE2SHORT) as i32, 0],
            forwardmove: want.forward,
            rightmove: want.right,
            ..UserCmd::default()
        };
        let context = pmove_context(
            0,
            (1.0, 1.0, 1.0),
            false,
            false,
            1.0 / 200.0,
            1.0 / 200.0,
            true,
            false,
            0,
            0,
            0,
            false,
            tuning,
        );
        pm_move(
            &mut ps,
            &mut cmd,
            context,
            backend,
            &FlatMantleAnimLength::default(),
            &ZeroMantleRootDelta,
        );
    }
    ps
}

fn run_policy(
    backend: &super::ClipBackend<'_>,
    mut ps: PlayerState,
    tuning: MovementTuning,
    ticks: usize,
    mut policy: impl FnMut(usize, &PlayerState) -> Intent,
) -> (PlayerState, Vec<([f32; 3], [f32; 3])>) {
    let mut frames = Vec::with_capacity(ticks);
    let mut old_buttons = 0;
    for tick in 0..ticks {
        let want = policy(tick, &ps);
        let held = if want.jump { buttons::JUMP } else { 0 };
        let mut cmd = UserCmd {
            server_time: ps.command_time + TICK_MS,
            angles: [0, (want.yaw * ANGLE2SHORT) as i32, 0],
            forwardmove: want.forward,
            rightmove: want.right,
            buttons: held,
            ..UserCmd::default()
        };
        let context = pmove_context(
            old_buttons,
            (1.0, 1.0, 1.0),
            false,
            false,
            1.0 / 200.0,
            1.0 / 200.0,
            true,
            false,
            0,
            0,
            0,
            false,
            tuning,
        );
        pm_move(
            &mut ps,
            &mut cmd,
            context,
            backend,
            &FlatMantleAnimLength::default(),
            &ZeroMantleRootDelta,
        );
        old_buttons = held;
        frames.push((ps.origin, ps.velocity));
    }
    (ps, frames)
}

fn course_brushes() -> Vec<crate::world::SimBrush> {
    let text = include_str!("../../../layouts/highrise_playground.json");
    map_layout::Layout::parse(text)
        .expect("the shipped layout parses")
        .brushes()
        .into_iter()
        .map(|brush| solid(brush.planes))
        .collect()
}

/// The shipped playground is traversable: ride both ramps, land on the mid
/// deck, cross the hop line and the corridor and reach the end-deck portal —
/// bunny-hopping or walking. Reads `layouts/highrise_playground.json`, so it
/// guards the course geometry against future edits.
#[test]
fn playground_course_reaches_the_end() {
    for hopping in [true, false] {
        let brushes = course_brushes();
        let backend = layout_backend(&brushes);
        let layout =
            map_layout::Layout::parse(include_str!("../../../layouts/highrise_playground.json"))
                .unwrap();
        let mut ps = spawn_player_state(layout.spawns[0].origin, [0.0, 0.0, 0.0]);
        ps.command_time = 1_000;
        ps.jump_time = -10_000;
        ps.ground_entity_num = trace_iw4::ENTITYNUM_NONE as i32;

        let mut landed = false;
        let (_, frames) = run_policy(&backend, ps, surf_on(), 1100, |_, ps| {
            let grounded = ps.ground_entity_num != trace_iw4::ENTITYNUM_NONE as i32;
            if ps.origin[2] > 3300.0 {
                return Intent {
                    forward: 127,
                    ..Intent::default()
                };
            }
            if !landed && grounded && ps.origin[2] < 1950.0 && ps.origin[0] > 100.0 {
                landed = true;
            }
            if landed {
                Intent {
                    forward: 127,
                    jump: hopping,
                    ..Intent::default()
                }
            } else {
                Intent {
                    right: 127,
                    ..Intent::default()
                }
            }
        });
        let portal = frames
            .iter()
            .position(|(origin, _)| origin[0] >= 3000.0)
            .unwrap_or_else(|| {
                panic!(
                    "hopping={hopping}: never reached the end-deck portal (final {:?})",
                    frames.last().map(|(origin, _)| *origin)
                )
            });
        assert!(
            frames[..portal]
                .iter()
                .all(|(origin, _)| origin[2] > 1500.0),
            "hopping={hopping}: fell off the course before the portal"
        );
    }
}

#[test]
fn layout_box_is_standable() {
    let floor = [solid(vec![
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, -1.0, 64.0],
        [1.0, 0.0, 0.0, 512.0],
        [-1.0, 0.0, 0.0, 512.0],
        [0.0, 1.0, 0.0, 512.0],
        [0.0, -1.0, 0.0, 512.0],
    ])];
    let backend = layout_backend(&floor);
    let mut ps = spawn_player_state([0.0, 0.0, 40.0], [0.0, 0.0, 0.0]);
    ps.command_time = 1_000;
    ps.ground_entity_num = trace_iw4::ENTITYNUM_NONE as i32;
    let ps = run_backend(
        &backend,
        ps,
        MovementTuning::default(),
        60,
        Intent::default(),
    );
    assert!(
        ps.origin[2].abs() < 1.0,
        "should rest on top: {:?}",
        ps.origin
    );
    assert_ne!(ps.ground_entity_num, trace_iw4::ENTITYNUM_NONE as i32);
}

#[test]
fn layout_ramp_is_surfable_through_brush_trace() {
    let ramp = [layout_wedge()];
    let backend = layout_backend(&ramp);
    let start = || {
        // On the -y face, about half way up.
        let mut ps = spawn_player_state([0.0, -110.0, 80.0], [0.0, 0.0, 0.0]);
        ps.command_time = 1_000;
        ps.jump_time = -10_000;
        ps.ground_entity_num = trace_iw4::ENTITYNUM_NONE as i32;
        ps.velocity = [600.0, 0.0, 0.0];
        ps
    };
    let into_ramp = Intent {
        right: -127,
        ..Intent::default()
    };
    let retail = run_backend(&backend, start(), MovementTuning::default(), 90, into_ramp);
    let surf = run_backend(&backend, start(), surf_on(), 90, into_ramp);
    assert!(
        retail.origin[2] < -200.0,
        "retail should slide off: {:?}",
        retail.origin
    );
    assert!(
        surf.origin[2] > 20.0,
        "surf should stay on the ramp: {:?}",
        surf.origin
    );
    assert!(
        horizontal_speed(&surf) > 590.0,
        "surf lost speed: {:?}",
        surf.velocity
    );
}

fn dropped_at(origin: [f32; 3]) -> PlayerState {
    let mut ps = spawn_player_state(origin, [0.0, 0.0, 0.0]);
    ps.command_time = 1_000;
    ps.jump_time = -10_000;
    ps.ground_entity_num = trace_iw4::ENTITYNUM_NONE as i32;
    ps
}

/// Surf technique: only the strafe key into the ramp. Adding forward pulls
/// the capped air acceleration off the ramp and the player slides away.
#[test]
fn landing_on_a_ramp_holds_with_strafe_only() {
    let ramp = [layout_wedge()];
    let backend = layout_backend(&ramp);
    let into = Intent {
        right: -127,
        ..Intent::default()
    };
    let forward_and_into = Intent {
        forward: 127,
        ..into
    };
    let held = run_backend(
        &backend,
        dropped_at([0.0, -100.0, 250.0]),
        surf_on(),
        120,
        into,
    );
    let slid = run_backend(
        &backend,
        dropped_at([0.0, -100.0, 250.0]),
        surf_on(),
        120,
        forward_and_into,
    );
    assert!(
        held.origin[2] > 40.0,
        "strafe-only should hold: {:?}",
        held.origin
    );
    assert!(
        slid.origin[2] < -100.0,
        "forward+strafe should slide off: {:?}",
        slid.origin
    );
}

fn map_layout_brushes(json_shapes: &str) -> Vec<crate::world::SimBrush> {
    map_layout::Layout::parse(&format!(
        r#"{{ "name": "t", "base_map": "m", "shapes": [{json_shapes}] }}"#
    ))
    .expect("test layout parses")
    .brushes()
    .into_iter()
    .map(|brush| solid(brush.planes))
    .collect()
}

/// A ramp tilted by `drop` turns gravity into speed along its length.
#[test]
fn descending_layout_ramp_builds_speed() {
    let ramp = map_layout_brushes(
        r#"{ "type": "ramp", "center": [0, 0, 0], "length": 4000, "width": 256, "drop": 600 }"#,
    );
    let backend = layout_backend(&ramp);
    let into = Intent {
        right: -127,
        ..Intent::default()
    };
    let end = run_backend(
        &backend,
        dropped_at([-1700.0, -90.0, 500.0]),
        surf_on(),
        120,
        into,
    );
    let base_z = -600.0 * end.origin[0] / 4000.0;
    assert!(
        end.velocity[0] > 200.0,
        "no speed gained along the ramp: {:?}",
        end.velocity
    );
    assert!(
        end.origin[2] > base_z,
        "fell off the ramp: {:?}",
        end.origin
    );
}

fn wallrun_on() -> MovementTuning {
    MovementTuning {
        wallrun: true,
        ..MovementTuning::default()
    }
}

/// A tall wall along +x at y 92..108; the player flies alongside it at y 60.
fn wallrun_world() -> Vec<crate::world::SimBrush> {
    map_layout_brushes(r#"{ "type": "box", "center": [0, 100, 500], "size": [4000, 16, 1000] }"#)
}

fn wallrun_start() -> PlayerState {
    let mut ps = spawn_player_state([0.0, 60.0, 500.0], [0.0, 0.0, 0.0]);
    ps.command_time = 1_000;
    ps.jump_time = -100_000;
    ps.ground_entity_num = trace_iw4::ENTITYNUM_NONE as i32;
    ps.velocity = [400.0, 0.0, 0.0];
    ps
}

/// Wall-running grips the wall beside the player and holds their height.
#[test]
fn wallrun_holds_height_along_a_wall() {
    let brushes = wallrun_world();
    let backend = layout_backend(&brushes);
    let (end, frames) = run_policy(&backend, wallrun_start(), wallrun_on(), 60, |tick, _| {
        if tick == 0 {
            Intent {
                jump: true,
                ..Intent::default()
            }
        } else {
            Intent::default()
        }
    });
    assert!(
        frames
            .iter()
            .skip(5)
            .all(|(origin, _)| (origin[2] - 500.0).abs() < 5.0),
        "wall-run should hold height: {:?}",
        frames.last()
    );
    assert!(
        end.origin[0] > 300.0,
        "wall-run should travel along the wall: {:?}",
        end.origin
    );
}

/// Without the tuning the same run falls; a wall alone does not carry you.
#[test]
fn retail_along_a_wall_still_falls() {
    let brushes = wallrun_world();
    let backend = layout_backend(&brushes);
    let (end, _) = run_policy(
        &backend,
        wallrun_start(),
        MovementTuning::default(),
        60,
        |_, _| Intent::default(),
    );
    assert!(
        end.origin[2] < 300.0,
        "retail should fall: {:?}",
        end.origin
    );
}

/// A jump edge while on the wall launches away from it and ends the run.
#[test]
fn wall_jump_pushes_away_from_the_wall() {
    let brushes = wallrun_world();
    let backend = layout_backend(&brushes);
    let (end, frames) = run_policy(&backend, wallrun_start(), wallrun_on(), 40, |tick, ps| {
        if tick == 0 || (tick >= 5 && (ps.pm_flags & movement_iw4::PMF_WALLRUN) != 0) {
            Intent {
                jump: true,
                ..Intent::default()
            }
        } else {
            Intent::default()
        }
    });
    assert!(
        frames[5].1[1] < -200.0,
        "wall jump should push away from the wall: {:?}",
        frames[5].1
    );
    assert_eq!(end.pm_flags & movement_iw4::PMF_WALLRUN, 0);
    assert!(end.origin[1] < 60.0, "left the wall: {:?}", end.origin);
}

/// In a corridor you have to jump to get airborne, so a fresh jump must not
/// lock the wall-run out for the whole hop.
#[test]
fn wallrun_grips_soon_after_a_jump() {
    let brushes = wallrun_world();
    let backend = layout_backend(&brushes);
    let mut ps = wallrun_start();
    ps.jump_time = ps.command_time;
    let (end, _) = run_policy(&backend, ps, wallrun_on(), 30, |tick, _| {
        if tick == 0 {
            Intent {
                jump: true,
                ..Intent::default()
            }
        } else {
            Intent::default()
        }
    });
    assert!(
        end.origin[2] > 490.0,
        "should have gripped after the jump instead of falling: {:?}",
        end.origin
    );
}

fn double_jump_on() -> MovementTuning {
    MovementTuning {
        double_jump: true,
        ..MovementTuning::default()
    }
}

/// One extra jump in the air: the first press launches, a second does nothing.
#[test]
fn double_jump_fires_once_in_the_air() {
    let brushes = map_layout_brushes("");
    let backend = layout_backend(&brushes);
    let mut ps = spawn_player_state([0.0, 0.0, 1000.0], [0.0, 0.0, 0.0]);
    ps.command_time = 1_000;
    ps.jump_time = -100_000;
    ps.ground_entity_num = trace_iw4::ENTITYNUM_NONE as i32;
    ps.velocity = [0.0, 0.0, -200.0];

    let (_, frames) = run_policy(&backend, ps, double_jump_on(), 10, |tick, _| {
        if tick == 0 || tick == 5 {
            Intent {
                jump: true,
                ..Intent::default()
            }
        } else {
            Intent::default()
        }
    });
    assert!(
        frames[0].1[2] > 200.0,
        "the air jump should launch upward: {:?}",
        frames[0].1
    );
    assert!(
        frames[5].1[2] < 200.0,
        "a second air jump must not fire: {:?}",
        frames[5].1
    );
}

/// The latch clears when the player is grounded, so the next hop has its jump.
#[test]
fn double_jump_latch_resets() {
    let mut ps = spawn_player_state([0.0, 0.0, 100.0], [0.0, 0.0, 0.0]);
    ps.gravity = 800;
    ps.ground_entity_num = trace_iw4::ENTITYNUM_NONE as i32;
    let mut cmd = UserCmd {
        buttons: 0x400,
        ..UserCmd::default()
    };
    let context = movement_iw4::DoubleJumpContext {
        jump_height: 39.0,
        old_buttons: 0,
    };
    assert!(movement_iw4::pm_double_jump(&mut ps, &cmd, context));
    assert!(ps.velocity[2] > 200.0, "launched: {:?}", ps.velocity);
    assert!(!movement_iw4::pm_double_jump(&mut ps, &cmd, context));
    movement_iw4::double_jump_reset(&mut ps);
    cmd.buttons = 0x400;
    assert!(movement_iw4::pm_double_jump(&mut ps, &cmd, context));
}

/// A bunny hop down a street must keep its speed: the wall-run only starts on a
/// deliberate jump press, not from brushing a wall mid-hop.
#[test]
fn hop_along_a_wall_is_not_hijacked() {
    let brushes = map_layout_brushes(
        r#"{ "type": "box", "center": [0, 0, -8], "size": [6000, 6000, 16] },
           { "type": "box", "center": [0, 100, 500], "size": [6000, 16, 1000] }"#,
    );
    let backend = layout_backend(&brushes);
    let mut ps = spawn_player_state([-2000.0, 60.0, 1.0], [0.0, 0.0, 0.0]);
    ps.command_time = 1_000;
    ps.jump_time = -100_000;
    ps.ground_entity_num = trace_iw4::ENTITYNUM_NONE as i32;
    ps.velocity = [300.0, 0.0, 0.0];
    let (end, _) = run_policy(&backend, ps, MovementTuning::fluid(), 300, |_, _| Intent {
        forward: 127,
        jump: true,
        ..Intent::default()
    });
    assert!(
        horizontal_speed(&end) > 280.0,
        "the hop should keep its speed past the wall: {:?}",
        end.velocity
    );
    assert_eq!(end.pm_flags & movement_iw4::PMF_WALLRUN, 0);
}
