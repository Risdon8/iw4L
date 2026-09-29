use bevy::prelude::*;

/// Local skating presentation. Physics remains owned by the optional Skate host.
#[derive(Resource, Default)]
pub struct SkateMode {
    pub active: bool,
    pub entering: bool,
    pub preloaded: bool,
    pub preload_pending: bool,
    pub controller: Option<usize>,
    pub pause_requested: bool,
    pub toggle_requested: bool,
    pub input_blocked: bool,
    pub client: u32,
    pub root: Mat4,
    pub bones: Vec<Mat4>,
    pub names: Vec<String>,
    pub camera: Option<(Transform, f32)>,
    pub tick: u64,
    pub status: String,

    /// Score/trick publication from the Skate host.
    pub score_total: f32,
    pub score_line: f32,
    pub score_sequence: f32,
    pub score_multiplier: f32,
    pub score_combo_fraction: f32,
    pub trick: String,
    pub trick_active: bool,
    pub score_bailed: bool,

    /// Horizontal board speed in game units per second (inches/s).
    pub speed_u_per_s: f32,
}
