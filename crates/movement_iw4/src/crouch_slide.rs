//! Mod: a crouch/power slide — tap crouch at speed to slide, keeping momentum.
//!
//! State rides in replicated `PlayerState` fields (`pm_flags`, `pm_time`), the
//! same way as wall-running, so prediction and replay agree.

use playerstate_iw4::{ENTITYNUM_NONE, PlayerState, UserCmd};

use crate::{CollisionBackend, MoveBounds, Pml, step_slide_move};

/// Set while sliding. Free `pm_flags` bits (`pm_drop_timers` clears `0x2180`).
pub const PMF_SLIDING: u32 = 0x0200_0000;
pub const PMF_SLIDE_COOLDOWN: u32 = 0x0400_0000;
/// A crouch press made in the air, buffered until the player lands.
pub const PMF_SLIDE_QUEUED: u32 = 0x0800_0000;

const BUTTON_CROUCH: u32 = 0x200;
const BUTTON_JUMP: u32 = 0x400;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlideContext {
    pub max_time_ms: i32,

    pub cooldown_ms: i32,

    /// Speed needed to start a slide.
    pub min_speed: f32,

    /// The slide ends once it drops below this.
    pub end_speed: f32,

    /// Fraction of horizontal speed shed per second while sliding.
    pub friction: f32,

    /// How hard the player can steer while sliding.
    pub steer_accel: f32,

    pub jump_up: f32,

    pub old_buttons: u32,
}

/// Runs the slide for this tick. Returns `true` when it moved the player, in
/// which case the ordinary walk move is skipped.
pub fn pm_crouch_slide<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &mut Pml,
    cmd: &UserCmd,
    context: SlideContext,
    bounds: MoveBounds,
    collision: &C,
) -> bool {
    if (ps.pm_flags & PMF_SLIDE_COOLDOWN) != 0 && ps.pm_time <= 0 {
        ps.pm_flags &= !PMF_SLIDE_COOLDOWN;
    }
    if (ps.pm_flags & PMF_SLIDING) != 0 {
        continue_slide(ps, pml, cmd, context, bounds, collision)
    } else {
        try_start(ps, pml, cmd, context, bounds, collision)
    }
}

/// While airborne, a crouch press queues a slide for the landing, so a jump can
/// lead straight into one.
pub fn pm_slide_air_intent(ps: &mut PlayerState, cmd: &UserCmd, old_buttons: u32) {
    if (cmd.buttons & BUTTON_CROUCH) != 0 && (old_buttons & BUTTON_CROUCH) == 0 {
        ps.pm_flags |= PMF_SLIDE_QUEUED;
    }
}

fn try_start<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &Pml,
    cmd: &UserCmd,
    context: SlideContext,
    bounds: MoveBounds,
    collision: &C,
) -> bool {
    let queued = (ps.pm_flags & PMF_SLIDE_QUEUED) != 0;
    let pressed = (cmd.buttons & BUTTON_CROUCH) != 0 && (context.old_buttons & BUTTON_CROUCH) == 0;
    if !queued && !pressed {
        return false;
    }
    // Consume the buffered press either way, so it cannot fire later by surprise.
    ps.pm_flags &= !PMF_SLIDE_QUEUED;
    if (ps.pm_flags & PMF_SLIDE_COOLDOWN) != 0 {
        return false;
    }
    if horizontal_speed(ps) < context.min_speed {
        return false;
    }
    ps.pm_flags |= PMF_SLIDING;
    ps.pm_time = context.max_time_ms;
    slide_move(ps, pml, cmd, context, bounds, collision);
    true
}

fn continue_slide<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &mut Pml,
    cmd: &UserCmd,
    context: SlideContext,
    bounds: MoveBounds,
    collision: &C,
) -> bool {
    if ps.pm_time <= 0 || horizontal_speed(ps) < context.end_speed {
        end_slide(ps, context);
        return false;
    }
    if (cmd.buttons & BUTTON_JUMP) != 0 && (context.old_buttons & BUTTON_JUMP) == 0 {
        // Jump out of it, keeping the horizontal momentum. Clearing the ground
        // state first is what lets the launch leave the floor instead of being
        // stepped back down.
        ps.ground_entity_num = ENTITYNUM_NONE;
        ps.jump_origin_z = ps.origin[2];
        ps.jump_time = cmd.server_time;
        ps.velocity[2] = context.jump_up;
        pml.walking = 0;
        pml.ground_plane = 0;
        pml.almost_ground_plane = 0;
        end_slide(ps, context);
        step_slide_move(
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
    slide_move(ps, &*pml, cmd, context, bounds, collision);
    true
}

fn slide_move<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &Pml,
    cmd: &UserCmd,
    context: SlideContext,
    bounds: MoveBounds,
    collision: &C,
) {
    let keep = 1.0 - context.friction * pml.frametime;
    let keep = if keep < 0.0 { 0.0 } else { keep };
    ps.velocity[0] *= keep;
    ps.velocity[1] *= keep;

    if context.steer_accel > 0.0 {
        let mut forward = pml.forward;
        let mut right = pml.right;
        forward[2] = 0.0;
        right[2] = 0.0;
        normalize(&mut forward);
        normalize(&mut right);
        let mut wishdir = [
            (cmd.rightmove as f32) * right[0] + (cmd.forwardmove as f32) * forward[0],
            (cmd.rightmove as f32) * right[1] + (cmd.forwardmove as f32) * forward[1],
            0.0,
        ];
        if normalize(&mut wishdir) > 0.0 {
            let add = context.steer_accel * pml.frametime;
            ps.velocity[0] += wishdir[0] * add;
            ps.velocity[1] += wishdir[1] * add;
        }
    }

    // Gravity along the ground: clipped away on the flat, downhill it becomes
    // speed, which is what makes a slide carry.
    ps.velocity[2] -= (ps.gravity as f32) * pml.frametime;
    clip_to_ground_plane(&mut ps.velocity, &pml.ground_trace[1..4]);
    step_slide_move(
        ps,
        pml,
        collision,
        bounds.mins,
        bounds.maxs,
        bounds.tracemask,
        None,
    );
}

fn end_slide(ps: &mut PlayerState, context: SlideContext) {
    ps.pm_flags &= !PMF_SLIDING;
    ps.pm_flags |= PMF_SLIDE_COOLDOWN;
    ps.pm_time = context.cooldown_ms;
}

fn horizontal_speed(ps: &PlayerState) -> f32 {
    libm::sqrtf(ps.velocity[0] * ps.velocity[0] + ps.velocity[1] * ps.velocity[1])
}

fn normalize(vector: &mut [f32; 3]) -> f32 {
    let length = libm::sqrtf(vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]);
    let divisor = if length <= 0.0 { 1.0 } else { length };
    let scale = 1.0 / divisor;
    vector[0] *= scale;
    vector[1] *= scale;
    vector[2] *= scale;
    length
}

fn clip_to_ground_plane(vector: &mut [f32; 3], normal: &[u32]) {
    let normal = [
        f32::from_bits(normal[0]),
        f32::from_bits(normal[1]),
        f32::from_bits(normal[2]),
    ];
    crate::project_velocity(vector, &normal);
}
