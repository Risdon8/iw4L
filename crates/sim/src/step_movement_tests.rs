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
