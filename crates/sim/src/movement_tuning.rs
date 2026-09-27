//! Mod: world-wide movement tuning (surf mode).
//!
//! Lives in `SimState` and travels in `SnapshotMeta`, so the authority, the
//! predicting client and a replay all step with the same values. It changes
//! only through `ClientAction::SetMovementTuning`.

use movement_iw4::SurfAirContext;

/// CS surf servers run `sv_airaccelerate` 100–150 with Source's fixed 30u cap.
pub const SURF_DEFAULT_AIR_ACCEL: f32 = 100.0;
pub const SURF_DEFAULT_AIR_WISHSPEED_CAP: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovementTuning {
    pub surf: bool,

    pub surf_air_accel: f32,

    pub surf_air_wishspeed_cap: f32,
}

impl Default for MovementTuning {
    fn default() -> Self {
        Self {
            surf: false,
            surf_air_accel: SURF_DEFAULT_AIR_ACCEL,
            surf_air_wishspeed_cap: SURF_DEFAULT_AIR_WISHSPEED_CAP,
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
        }
    }
}
