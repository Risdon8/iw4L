//! What the stock IW4 `_gamelogic` scripts mean to the match. Everything here
//! depends on the stock script layout, not on the VM.

use super::runtime::{level_endon_armed, raise, return_from};
use super::*;
use crate::frame::FrameWorld;
use crate::identities::MatchPhase;
use bevy_ecs::prelude::World;

/// A level notify the stock scripts send when the match state changes.
pub(crate) fn apply_level_notify(frame: &mut FrameWorld, name: &str) {
    if name == "prematch_over" && frame.phase() == MatchPhase::Warmup {
        crate::score::finish_prematch(frame);
    }
}

const WAIT_FOR_PLAYERS: &str = "maps/mp/gametypes/_gamelogic::waitforplayers";
const START_TIMER_BEGINNING: &str = "match_start_timer_beginning";

/// Debug override: the stock scripts expose no entry point that ends prematch,
/// so this returns the scripts out of their player wait and cancels the start
/// countdown. Returns false when no prematch script is running.
pub(crate) fn force_match_start(world: &mut World, tick: crate::Tick) -> bool {
    let runtime = world.resource::<Runtime>();
    if runtime.fault.is_some() || runtime.program.is_none() {
        return false;
    }
    let counting = level_endon_armed(world, START_TIMER_BEGINNING);
    let now = i64::from(tick.0) * i64::from(crate::MATCH_TICK_MS);
    let waiting = return_from(world, WAIT_FOR_PLAYERS, now);
    if !counting && waiting == 0 {
        return false;
    }
    world
        .resource_mut::<Runtime>()
        .set_object_field(0, "prematchperiodend", Value::Int(0));
    raise(world, Value::level(), START_TIMER_BEGINNING, Vec::new());
    true
}
