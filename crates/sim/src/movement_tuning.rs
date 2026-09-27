//! Mod: world-wide movement tuning (surf mode).
//!
//! Lives in `SimState` and travels in `SnapshotMeta`, so the authority, the
//! predicting client and a replay all step with the same values. It changes
//! only through `ClientAction::SetMovementTuning`.

use movement_iw4::{SurfAirContext, WallRunContext};

/// CS surf servers run `sv_airaccelerate` 100–150 with Source's fixed 30u cap.
pub const SURF_DEFAULT_AIR_ACCEL: f32 = 100.0;
pub const SURF_DEFAULT_AIR_WISHSPEED_CAP: f32 = 30.0;

/// A wall-run holds for about two and a half seconds, then needs a moment off
/// the wall before it can start again.
pub const WALLRUN_DEFAULT_TIME_MS: i32 = 2500;
pub const WALLRUN_DEFAULT_COOLDOWN_MS: i32 = 600;
pub const WALLRUN_DEFAULT_MIN_SPEED: f32 = 200.0;
pub const WALLRUN_DEFAULT_JUMP_UP: f32 = 250.0;
pub const WALLRUN_DEFAULT_JUMP_OUT: f32 = 260.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovementTuning {
    pub surf: bool,

    pub surf_air_accel: f32,

    pub surf_air_wishspeed_cap: f32,

    pub wallrun: bool,

    pub wallrun_time_ms: i32,

    pub wallrun_cooldown_ms: i32,

    pub wallrun_min_speed: f32,

    pub wallrun_jump_up: f32,

    pub wallrun_jump_out: f32,
}

impl Default for MovementTuning {
    fn default() -> Self {
        Self {
            surf: false,
            surf_air_accel: SURF_DEFAULT_AIR_ACCEL,
            surf_air_wishspeed_cap: SURF_DEFAULT_AIR_WISHSPEED_CAP,
            wallrun: false,
            wallrun_time_ms: WALLRUN_DEFAULT_TIME_MS,
            wallrun_cooldown_ms: WALLRUN_DEFAULT_COOLDOWN_MS,
            wallrun_min_speed: WALLRUN_DEFAULT_MIN_SPEED,
            wallrun_jump_up: WALLRUN_DEFAULT_JUMP_UP,
            wallrun_jump_out: WALLRUN_DEFAULT_JUMP_OUT,
        }
    }
}

impl MovementTuning {
    pub fn surf_air(&self) -> Option<SurfAirContext> {
        self.surf.then_some(SurfAirContext {
            accel: self.surf_air_accel,
            wishspeed_cap: self.surf_air_wishspeed_cap,
        })
    }

    pub fn wallrun(&self, old_buttons: u32) -> Option<WallRunContext> {
        self.wallrun.then_some(WallRunContext {
            max_time_ms: self.wallrun_time_ms,
            cooldown_ms: self.wallrun_cooldown_ms,
            min_speed: self.wallrun_min_speed,
            trace_dist: WALLRUN_TRACE_DIST,
            jump_up: self.wallrun_jump_up,
            jump_out: self.wallrun_jump_out,
            gravity_scale: 0.0,
            old_buttons,
        })
    }

    /// Clamp values arriving from the wire or the console into a range that
    /// cannot produce NaN velocities or a frozen player.
    pub fn sanitized(self) -> Self {
        let finite = |value: f32, fallback: f32| if value.is_finite() { value } else { fallback };
        Self {
            surf: self.surf,
            surf_air_accel: finite(self.surf_air_accel, SURF_DEFAULT_AIR_ACCEL)
                .clamp(0.0, 10_000.0),
            surf_air_wishspeed_cap: finite(
                self.surf_air_wishspeed_cap,
                SURF_DEFAULT_AIR_WISHSPEED_CAP,
            )
            .clamp(0.0, 10_000.0),
            wallrun: self.wallrun,
            wallrun_time_ms: self.wallrun_time_ms.clamp(0, 60_000),
            wallrun_cooldown_ms: self.wallrun_cooldown_ms.clamp(0, 60_000),
            wallrun_min_speed: finite(self.wallrun_min_speed, WALLRUN_DEFAULT_MIN_SPEED)
                .clamp(0.0, 10_000.0),
            wallrun_jump_up: finite(self.wallrun_jump_up, WALLRUN_DEFAULT_JUMP_UP)
                .clamp(0.0, 10_000.0),
            wallrun_jump_out: finite(self.wallrun_jump_out, WALLRUN_DEFAULT_JUMP_OUT)
                .clamp(0.0, 10_000.0),
        }
    }
}

/// How far to the side a wall is looked for. Constant for now.
const WALLRUN_TRACE_DIST: f32 = 40.0;
