use bevy::prelude::Resource;

/// A run through the active layout course: elapsed time, best, and splits.
///
/// Filled on the host by the console's timer system (which reads the sim's
/// checkpoint progress) and read by the HUD.
#[derive(Resource, Default)]
pub struct RunTimer {
    pub running: bool,
    pub elapsed: f32,
    /// Best completed run for the current course.
    pub best: Option<f32>,
    /// Splits taken this run, in order.
    pub splits: Vec<RunSplit>,
    /// Most recent split, for the HUD's flash.
    pub last_split: Option<RunSplit>,
    /// Checkpoints in the active course (0 = none).
    pub checkpoint_count: usize,
    /// The course the timer is following.
    pub course: String,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct RunSplit {
    pub label: String,
    pub time: f32,
    /// Time minus the best for this checkpoint (negative = ahead).
    pub delta: Option<f32>,
}
