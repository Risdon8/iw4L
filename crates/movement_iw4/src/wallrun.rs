//! Mod: Titanfall-style wall-running.
//!
//! A player who flies alongside a near-vertical wall sticks to it, holds their
//! height for a limited time and can jump off. Tunables travel in
//! `MovementTuning`; the live state rides in `PlayerState` fields that already
//! replicate (`pm_flags`, `pm_time`, `v_ladder_vec`, `jump_time`), so the
//! authority, a predicting client and a replay all agree.

use playerstate_iw4::{PlayerState, UserCmd};

use crate::{CollisionBackend, GroundTraceInput, MoveBounds, Pml, pm_step_slide_move};

/// Set while the player is on a wall. A free `pm_flags` bit (the retail mask
/// `pm_drop_timers` clears, `0x2180`, does not include it).
pub const PMF_WALLRUN: u32 = 0x0020_0000;

/// Set while the wall-run cooldown is counting down; `pm_time` holds the ms.
/// A normal jump must not block a wall-run, so the cooldown is separate from
/// `jump_time`.
pub const PMF_WALLRUN_COOLDOWN: u32 = 0x0080_0000;

const BUTTON_JUMP: u32 = 0x400;

/// A surface steeper than this is a wall; a walkable floor or ramp is not.
const WALL_MAX_NORMAL_Z: f32 = 0.3;

/// The wall is probed at chest height, so the floor never answers.
const PROBE_HEIGHT: f32 = 35.0;
const PROBE_HALF: f32 = 2.0;

/// How much of the tangential speed survives a wall jump.
const WALL_JUMP_KEEP: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallRunContext {
    /// How long one wall-run lasts.
    pub max_time_ms: i32,

    /// No attach until this long after the last jump or wall-run.
    pub cooldown_ms: i32,

    /// Horizontal speed needed to start and keep a run.
    pub min_speed: f32,

    /// How far to look sideways for a wall.
    pub trace_dist: f32,

    /// Upward launch of a wall jump.
    pub jump_up: f32,

    /// Push away from the wall on a wall jump.
    pub jump_out: f32,

    /// Gravity while on the wall. `0` holds the player's height.
    pub gravity_scale: f32,

    pub old_buttons: u32,
}

/// Runs the wall-run for this tick. Returns `true` when it moved the player, in
/// which case the ordinary air move is skipped.
pub fn pm_wallrun<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &Pml,
    cmd: &UserCmd,
    context: WallRunContext,
    bounds: MoveBounds,
    collision: &C,
) -> bool {
    if (ps.pm_flags & PMF_WALLRUN_COOLDOWN) != 0 && ps.pm_time <= 0 {
        ps.pm_flags &= !PMF_WALLRUN_COOLDOWN;
    }
    if (ps.pm_flags & PMF_WALLRUN) != 0 {
        continue_run(ps, pml, cmd, context, bounds, collision)
    } else {
        try_attach(ps, pml, cmd, context, bounds, collision)
    }
}

fn try_attach<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &Pml,
    cmd: &UserCmd,
    context: WallRunContext,
    bounds: MoveBounds,
    collision: &C,
) -> bool {
    if (ps.pm_flags & PMF_WALLRUN_COOLDOWN) != 0 {
        return false;
    }
    // Deliberate: a fresh jump press beside the wall starts the run, so an
    // ordinary hop down a street is not hijacked by every wall it passes.
    if (cmd.buttons & BUTTON_JUMP) == 0 || (context.old_buttons & BUTTON_JUMP) != 0 {
        return false;
    }
    let speed = libm::sqrtf(ps.velocity[0] * ps.velocity[0] + ps.velocity[1] * ps.velocity[1]);
    if speed < context.min_speed {
        return false;
    }
    let Some(normal) = probe_sides(ps, pml, context.trace_dist, bounds, collision) else {
        return false;
    };
    let (tangent, tangent_speed) = along_wall(ps.velocity, normal);
    if tangent_speed < context.min_speed {
        return false;
    }

    ps.pm_flags |= PMF_WALLRUN;
    ps.pm_time = context.max_time_ms;
    ps.v_ladder_vec = normal;
    ps.velocity[0] = tangent[0];
    ps.velocity[1] = tangent[1];
    hold_height(ps, pml, context, bounds, collision);
    true
}

fn continue_run<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &Pml,
    cmd: &UserCmd,
    context: WallRunContext,
    bounds: MoveBounds,
    collision: &C,
) -> bool {
    if ps.pm_time <= 0 {
        detach(ps, context.cooldown_ms);
        return false;
    }
    let stored = ps.v_ladder_vec;
    let Some(normal) = probe_matching(ps, pml, context.trace_dist, stored, bounds, collision)
    else {
        detach(ps, context.cooldown_ms);
        return false;
    };

    if (cmd.buttons & BUTTON_JUMP) != 0 && (context.old_buttons & BUTTON_JUMP) == 0 {
        let (tangent, _) = along_wall(ps.velocity, normal);
        ps.velocity[0] = tangent[0] * WALL_JUMP_KEEP + normal[0] * context.jump_out;
        ps.velocity[1] = tangent[1] * WALL_JUMP_KEEP + normal[1] * context.jump_out;
        ps.velocity[2] = context.jump_up;
        detach(ps, context.cooldown_ms);
        // The launch moves this tick under ordinary gravity.
        pm_step_slide_move(
            ps,
            pml,
            collision,
            bounds.mins,
            bounds.maxs,
            bounds.tracemask,
            Some(ps.gravity as f32),
        );
        return true;
    }

    ps.v_ladder_vec = normal;
    let (tangent, _) = along_wall(ps.velocity, normal);
    ps.velocity[0] = tangent[0];
    ps.velocity[1] = tangent[1];
    hold_height(ps, pml, context, bounds, collision);
    true
}

/// Keeps the player level while on the wall: no vertical speed, gravity scaled
/// (zero by default) so the run cannot descend.
fn hold_height<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &Pml,
    context: WallRunContext,
    bounds: MoveBounds,
    collision: &C,
) {
    ps.velocity[2] = 0.0;
    let gravity = ps.gravity as f32 * context.gravity_scale;
    pm_step_slide_move(
        ps,
        pml,
        collision,
        bounds.mins,
        bounds.maxs,
        bounds.tracemask,
        Some(gravity),
    );
}

fn detach(ps: &mut PlayerState, cooldown_ms: i32) {
    ps.pm_flags &= !PMF_WALLRUN;
    ps.pm_flags |= PMF_WALLRUN_COOLDOWN;
    ps.pm_time = cooldown_ms;
    ps.v_ladder_vec = [0.0; 3];
}

/// Velocity with the into-wall component removed, and its horizontal length.
fn along_wall(velocity: [f32; 3], normal: [f32; 3]) -> ([f32; 3], f32) {
    let into = velocity[0] * normal[0] + velocity[1] * normal[1];
    let tangent = [
        velocity[0] - into * normal[0],
        velocity[1] - into * normal[1],
        velocity[2],
    ];
    let speed = libm::sqrtf(tangent[0] * tangent[0] + tangent[1] * tangent[1]);
    (tangent, speed)
}

fn probe_sides<C: CollisionBackend>(
    ps: &PlayerState,
    pml: &Pml,
    dist: f32,
    bounds: MoveBounds,
    collision: &C,
) -> Option<[f32; 3]> {
    let right = flat_right(pml);
    probe(ps, right, dist, bounds, collision)
        .or_else(|| probe(ps, [-right[0], -right[1], 0.0], dist, bounds, collision))
}

fn probe_matching<C: CollisionBackend>(
    ps: &PlayerState,
    pml: &Pml,
    dist: f32,
    stored: [f32; 3],
    bounds: MoveBounds,
    collision: &C,
) -> Option<[f32; 3]> {
    let right = flat_right(pml);
    let sides = [right, [-right[0], -right[1], 0.0]];
    for dir in sides {
        if let Some(normal) = probe(ps, dir, dist, bounds, collision)
            && normal[0] * stored[0] + normal[1] * stored[1] > 0.5
        {
            return Some(normal);
        }
    }
    None
}

fn probe<C: CollisionBackend>(
    ps: &PlayerState,
    dir: [f32; 3],
    dist: f32,
    bounds: MoveBounds,
    collision: &C,
) -> Option<[f32; 3]> {
    let start = [ps.origin[0], ps.origin[1], ps.origin[2] + PROBE_HEIGHT];
    let end = [start[0] + dir[0] * dist, start[1] + dir[1] * dist, start[2]];
    let hit = collision.trace(GroundTraceInput {
        start,
        end,
        mins: [-PROBE_HALF; 3],
        maxs: [PROBE_HALF; 3],
        tracemask: bounds.tracemask,
    });
    if hit.fraction >= 1.0 {
        return None;
    }
    let normal = hit.normal;
    if libm::fabsf(normal[2]) > WALL_MAX_NORMAL_Z {
        return None;
    }
    // A wall the run can grip faces back toward the player.
    if normal[0] * dir[0] + normal[1] * dir[1] > -0.3 {
        return None;
    }
    Some(normal)
}

fn flat_right(pml: &Pml) -> [f32; 3] {
    let mut right = [pml.right[0], pml.right[1], 0.0];
    let length = libm::sqrtf(right[0] * right[0] + right[1] * right[1]);
    if length > 0.0 {
        right[0] /= length;
        right[1] /= length;
    }
    right
}
