//! Mod: one extra jump in the air, reset when the player lands.

use playerstate_iw4::{ENTITYNUM_NONE, PlayerState, UserCmd};

/// Latches once the air jump has been used this hop.
pub const PMF_DOUBLE_JUMP_USED: u32 = 0x0100_0000;

const BUTTON_JUMP: u32 = 0x400;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoubleJumpContext {
    /// Same meaning as the ground jump height.
    pub jump_height: f32,

    pub old_buttons: u32,
}

/// Applies the air jump on a fresh press. Returns `true` when it fired; the
/// caller still runs the ordinary air move so the launch integrates.
pub fn pm_double_jump(ps: &mut PlayerState, cmd: &UserCmd, context: DoubleJumpContext) -> bool {
    if ps.ground_entity_num != ENTITYNUM_NONE {
        return false;
    }
    if (ps.pm_flags & PMF_DOUBLE_JUMP_USED) != 0 {
        return false;
    }
    if (cmd.buttons & BUTTON_JUMP) == 0 || (context.old_buttons & BUTTON_JUMP) != 0 {
        return false;
    }

    ps.pm_flags |= PMF_DOUBLE_JUMP_USED;
    let energy = (ps.gravity as f32) * (context.jump_height + context.jump_height);
    ps.velocity[2] = if energy > 0.0 {
        libm::sqrtf(energy)
    } else {
        0.0
    };
    // Deliberately does not touch `jump_time`: that would lock the next ground
    // jump out for the jump gate's 500 ms after any air jump.
    true
}

/// Clears the latch; call whenever the player is on the ground.
pub fn double_jump_reset(ps: &mut PlayerState) {
    ps.pm_flags &= !PMF_DOUBLE_JUMP_USED;
}
