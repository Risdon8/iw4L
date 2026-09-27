//! Mod: runtime side of a map layout — extra collision and fall resets.
//!
//! Both live in `SimContent`, so the authority and the predicting client
//! trace and reset against the same shapes.

use crate::world::{ClientId, SimBrush};
use crate::{ClientLifecycle, frame::FrameWorld};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutRules {
    pub reset_below_z: Option<f32>,

    pub reset_volumes: Vec<([f32; 3], [f32; 3])>,

    /// Where a reset sends the player: origin and yaw.
    pub respawn: Vec<([f32; 3], f32)>,

    /// Contents bits removed from the player trace mask (the map's
    /// out-of-bounds player clip, for a course built in open air).
    pub ignore_contents: u32,
}

impl LayoutRules {
    fn wants_reset(&self, origin: [f32; 3]) -> bool {
        self.reset_below_z.is_some_and(|z| origin[2] < z)
            || self
                .reset_volumes
                .iter()
                .any(|(min, max)| (0..3).all(|i| origin[i] >= min[i] && origin[i] <= max[i]))
    }
}

pub(crate) fn layout_brush_refs(
    brushes: &[SimBrush],
) -> impl Iterator<Item = trace_iw4::BrushRef<'_>> {
    brushes.iter().map(|brush| trace_iw4::BrushRef {
        planes: &brush.planes,
        contents: brush.contents,
        plane_surface_flags: &brush.plane_surface_flags,
        glass_encoded: brush.glass_encoded,
    })
}

/// Merge a layout trace into a world trace: the nearer hit wins, and
/// start-solid from either side sticks.
pub(crate) fn merge_layout_hit(
    world_hit: trace_iw4::Trace,
    brushes: &[SimBrush],
    input: movement_iw4::GroundTraceInput,
) -> trace_iw4::Trace {
    if brushes.is_empty() {
        return world_hit;
    }
    let layout_hit = trace_iw4::trace_capsule(
        layout_brush_refs(brushes),
        input.start,
        input.end,
        input.mins,
        input.maxs,
        input.tracemask,
    );
    let startsolid = world_hit.startsolid | layout_hit.startsolid;
    let allsolid = world_hit.allsolid | layout_hit.allsolid;
    let mut hit = if layout_hit.fraction < world_hit.fraction {
        layout_hit
    } else {
        world_hit
    };
    hit.startsolid = startsolid;
    hit.allsolid = allsolid;
    hit
}

/// Keeps the player's view; only position and velocity change.
pub(crate) fn apply_reset(world: &mut FrameWorld, id: ClientId) {
    let content = world.content();
    let Some(rules) = content.layout_rules() else {
        return;
    };
    let Some(&(target, _yaw)) = rules.respawn.first() else {
        return;
    };
    if !world
        .client_meta(id)
        .is_some_and(|m| m.lifecycle == ClientLifecycle::Alive)
    {
        return;
    }
    let old_origin = {
        let Some(ps) = world.player_mut(id) else {
            return;
        };
        if !rules.wants_reset(ps.origin) {
            return;
        }
        let old = ps.origin;
        ps.origin = target;
        ps.velocity = [0.0, 0.0, 0.0];
        ps.e_flags ^= playerstate_iw4::eflags::TELEPORT;
        old
    };
    world.translate_player_area(
        id,
        [
            target[0] - old_origin[0],
            target[1] - old_origin[1],
            target[2] - old_origin[2],
        ],
    );
}

/// One player-shaped trace split by source, for authoring layouts.
#[derive(Clone, Copy, Debug)]
pub struct ClipProbe {
    pub map: trace_iw4::Trace,
    pub layout: trace_iw4::Trace,
    pub tracemask: u32,
}

impl crate::world::SimState {
    /// Traces the standing player box from `start` to `end` against the map
    /// (brushes and mesh, with the layout's clip mask applied) and against
    /// the layout shapes separately.
    pub fn probe_player_clip(&self, start: [f32; 3], end: [f32; 3]) -> ClipProbe {
        let content = self.content();
        let ignore = content.layout_rules().map_or(0, |r| r.ignore_contents);
        let tracemask = 0x0281_0011 & !ignore & !crate::world::CONTENTS_BODY;
        let mins = [-15.0, -15.0, 0.0];
        let maxs = [15.0, 15.0, 70.0];
        let map = crate::world::clip_trace(
            content.clip_brushes(),
            content.clip_bsp(),
            content.clip_mesh(),
            start,
            end,
            mins,
            maxs,
            tracemask,
            &|_| true,
        );
        let layout = trace_iw4::trace_capsule(
            layout_brush_refs(content.layout_brushes()),
            start,
            end,
            mins,
            maxs,
            tracemask,
        );
        ClipProbe {
            map,
            layout,
            tracemask,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_triggers_below_floor_and_in_volumes() {
        let rules = LayoutRules {
            reset_below_z: Some(100.0),
            reset_volumes: vec![([0.0, 0.0, 500.0], [10.0, 10.0, 510.0])],
            respawn: vec![([0.0, 0.0, 1000.0], 0.0)],
            ignore_contents: 0,
        };
        assert!(rules.wants_reset([0.0, 0.0, 99.0]));
        assert!(rules.wants_reset([5.0, 5.0, 505.0]));
        assert!(!rules.wants_reset([50.0, 5.0, 505.0]));
        assert!(!rules.wants_reset([0.0, 0.0, 101.0]));
    }
}
