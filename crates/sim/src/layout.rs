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

    /// Volumes that always send the player to the start, checkpoint or not.
    pub restart_volumes: Vec<([f32; 3], [f32; 3])>,

    /// Where a reset sends the player: origin and yaw.
    pub respawn: Vec<([f32; 3], f32)>,

    /// Ordered course sections a fall returns the player to.
    pub checkpoints: Vec<CheckpointRule>,

    /// Contents bits removed from the player trace mask (the map's
    /// out-of-bounds player clip, for a course built in open air).
    pub ignore_contents: u32,
}

/// One course section: standing in `volume` remembers `origin` as the respawn.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckpointRule {
    pub name: String,
    pub volume: ([f32; 3], [f32; 3]),
    pub origin: [f32; 3],
    pub yaw: f32,
}

fn in_volume(volume: &([f32; 3], [f32; 3]), p: [f32; 3]) -> bool {
    let (min, max) = *volume;
    (0..3).all(|i| p[i] >= min[i] && p[i] <= max[i])
}

impl LayoutRules {
    fn wants_reset(&self, origin: [f32; 3]) -> bool {
        self.reset_below_z.is_some_and(|z| origin[2] < z)
            || self
                .reset_volumes
                .iter()
                .any(|volume| in_volume(volume, origin))
    }

    fn wants_restart(&self, origin: [f32; 3]) -> bool {
        self.restart_volumes
            .iter()
            .any(|volume| in_volume(volume, origin))
    }

    /// The furthest section the player is standing in right now. Volumes run in
    /// course order, so the largest matching index is the newest.
    fn entered_checkpoint(&self, origin: [f32; 3]) -> Option<usize> {
        self.checkpoints
            .iter()
            .rposition(|checkpoint| in_volume(&checkpoint.volume, origin))
    }

    /// The start spawn.
    fn start(&self) -> Option<([f32; 3], f32)> {
        self.respawn.first().copied()
    }

    /// Where a fall sends the player: the held checkpoint, else the start.
    fn fall_target(&self, reached: Option<usize>) -> Option<([f32; 3], f32)> {
        reached
            .and_then(|index| self.checkpoints.get(index))
            .map(|checkpoint| (checkpoint.origin, checkpoint.yaw))
            .or_else(|| self.start())
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

/// Keeps the player's view; only position and velocity change. A fall returns
/// to the last checkpoint held this life; a restart volume always returns to
/// the start and forgets the checkpoints.
pub(crate) fn apply_reset(world: &mut FrameWorld, id: ClientId) {
    let content = world.content();
    let Some(rules) = content.layout_rules() else {
        return;
    };
    let Some(meta) = world.client_meta(id) else {
        return;
    };
    if meta.lifecycle != ClientLifecycle::Alive {
        return;
    }
    let life = meta.life_sequence;
    let Some(origin) = world.player(id).map(|ps| ps.origin) else {
        return;
    };

    if rules.wants_restart(origin) {
        world.client_meta_mut(id).layout_checkpoint = None;
        teleport(world, id, rules.start());
        return;
    }

    let reached = {
        let meta = world.client_meta_mut(id);
        match meta.layout_checkpoint {
            Some((stored, index)) if stored == life => Some(index),
            _ => None,
        }
    };
    // Standing in a section claims it — including an earlier one after the
    // `checkpoint` command jumps back to it.
    let reached = match rules.entered_checkpoint(origin) {
        Some(index) => {
            world.client_meta_mut(id).layout_checkpoint = Some((life, index));
            Some(index)
        }
        None => reached,
    };
    if rules.wants_reset(origin) {
        teleport(world, id, rules.fall_target(reached));
    }
}

/// Keeps the player's view; only position and velocity change.
fn teleport(world: &mut FrameWorld, id: ClientId, target: Option<([f32; 3], f32)>) {
    let Some((target, _yaw)) = target else {
        return;
    };
    let Some(ps) = world.player_mut(id) else {
        return;
    };
    let old = ps.origin;
    ps.origin = target;
    ps.velocity = [0.0, 0.0, 0.0];
    ps.e_flags ^= playerstate_iw4::eflags::TELEPORT;
    world.translate_player_area(
        id,
        [target[0] - old[0], target[1] - old[1], target[2] - old[2]],
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
    /// The layout checkpoint this client currently holds, for the `checkpoint`
    /// console command. `None` means a fall would return to the start.
    pub fn layout_checkpoint(&self, id: ClientId) -> Option<usize> {
        self.client_meta(id)
            .and_then(|meta| meta.layout_checkpoint)
            .map(|(_, index)| index)
    }

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

    fn rules() -> LayoutRules {
        LayoutRules {
            reset_below_z: Some(100.0),
            reset_volumes: vec![([0.0, 0.0, 500.0], [10.0, 10.0, 510.0])],
            restart_volumes: vec![([20.0, 20.0, 500.0], [30.0, 30.0, 510.0])],
            respawn: vec![([0.0, 0.0, 1000.0], 0.0)],
            checkpoints: vec![
                CheckpointRule {
                    name: "one".into(),
                    volume: ([0.0, 0.0, 2000.0], [50.0, 50.0, 2100.0]),
                    origin: [25.0, 25.0, 2000.0],
                    yaw: 0.0,
                },
                CheckpointRule {
                    name: "two".into(),
                    volume: ([100.0, 0.0, 2000.0], [150.0, 50.0, 2100.0]),
                    origin: [125.0, 25.0, 2000.0],
                    yaw: 90.0,
                },
            ],
            ignore_contents: 0,
        }
    }

    #[test]
    fn reset_triggers_below_floor_and_in_volumes() {
        let rules = rules();
        assert!(rules.wants_reset([0.0, 0.0, 99.0]));
        assert!(rules.wants_reset([5.0, 5.0, 505.0]));
        assert!(!rules.wants_reset([50.0, 5.0, 505.0]));
        assert!(!rules.wants_reset([0.0, 0.0, 101.0]));
    }

    #[test]
    fn restart_volumes_are_separate_from_fall_volumes() {
        let rules = rules();
        assert!(rules.wants_restart([25.0, 25.0, 505.0]));
        assert!(!rules.wants_reset([25.0, 25.0, 505.0]));
        assert!(rules.start().is_some());
    }

    #[test]
    fn entering_a_checkpoint_remembers_the_furthest_one() {
        let rules = rules();
        assert_eq!(rules.entered_checkpoint([10.0, 10.0, 2050.0]), Some(0));
        assert_eq!(rules.entered_checkpoint([125.0, 10.0, 2050.0]), Some(1));
        // Overlapping the two volumes reports the newer section.
        assert_eq!(rules.entered_checkpoint([101.0, 10.0, 2050.0]), Some(1));
        assert_eq!(rules.entered_checkpoint([500.0, 10.0, 2050.0]), None);
    }

    #[test]
    fn a_fall_lands_on_the_held_checkpoint_else_the_start() {
        let rules = rules();
        assert_eq!(rules.fall_target(None), Some(([0.0, 0.0, 1000.0], 0.0)));
        assert_eq!(
            rules.fall_target(Some(1)),
            Some(([125.0, 25.0, 2000.0], 90.0))
        );
        assert_eq!(
            rules.fall_target(Some(9)),
            Some(([0.0, 0.0, 1000.0], 0.0)),
            "an out-of-range index ignores checkpoints"
        );
    }
}
