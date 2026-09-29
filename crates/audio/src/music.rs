//! Local music player: plays tracks the user drops into `music/` beside the
//! executable.
//!
//! The player is client-local and survives map loads — its entities carry no
//! match [`crate::Voice`], so the audio backend never cancels them. It is
//! driven from the console (`music on|off|next|prev|volume`); the console sets
//! a [`MusicRequest`] or a setting and [`drive_music`] does the audio work.

use std::path::{Path, PathBuf};

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::pcm::{PcmAudio, decode_audio_bytes};

const DEFAULT_VOLUME: f32 = 1.0;
const START_TIMEOUT_SECS: f32 = 1.5;

const EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "flac"];

/// What happens when the current track ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repeat {
    /// Play the next track and wrap around at the end (the default).
    #[default]
    All,
    /// Stop after the last track.
    Off,
    /// Repeat the current track forever.
    One,
}

/// A one-shot action requested from the console.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MusicRequest {
    On,
    Off,
    Toggle,
    Next,
    Prev,
    Track(usize),
    Reload,
}

/// Marker on the entity holding the current track.
#[derive(Component)]
pub(crate) struct MusicTrack;

#[derive(Resource)]
pub struct MusicPlayer {
    dir: PathBuf,
    paths: Vec<PathBuf>,
    names: Vec<String>,
    index: usize,
    /// Whether playback should be running.
    pub enabled: bool,
    /// Linear gain, 0.0..=1.0.
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// Human-readable last result, shown by `music`.
    pub status: String,
    current: Option<Entity>,
    current_started: bool,
    current_elapsed: f32,
    applied_volume: f32,
    request: Option<MusicRequest>,
    rng: u64,
    scanned: bool,
}

impl Default for MusicPlayer {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("music"),
            paths: Vec::new(),
            names: Vec::new(),
            index: 0,
            enabled: false,
            volume: DEFAULT_VOLUME,
            shuffle: false,
            repeat: Repeat::default(),
            status: "music: not scanned yet".to_owned(),
            current: None,
            current_started: false,
            current_elapsed: 0.0,
            applied_volume: DEFAULT_VOLUME,
            request: None,
            rng: 0x2545_f491_4f6c_dd1d,
            scanned: false,
        }
    }
}

impl MusicPlayer {
    /// Folder the tracks are read from.
    pub fn directory(&self) -> &Path {
        &self.dir
    }

    pub fn track_count(&self) -> usize {
        self.names.len()
    }

    pub fn track_names(&self) -> &[String] {
        &self.names
    }

    pub fn track_name(&self, index: usize) -> Option<&str> {
        self.names.get(index).map(String::as_str)
    }

    pub fn current_index(&self) -> usize {
        self.index
    }

    pub fn now_playing(&self) -> Option<&str> {
        if self.current.is_some() {
            self.track_name(self.index)
        } else {
            None
        }
    }

    /// Queue an action for [`drive_music`] on the next frame.
    pub fn request(&mut self, request: MusicRequest) {
        self.request = Some(request);
    }

    /// A one-line summary for the console prompt.
    pub fn summary(&self) -> String {
        let state = if self.enabled {
            self.now_playing().unwrap_or("(starting)")
        } else {
            "(stopped)"
        };
        let mut line = format!(
            "music: {} — {} | {} track(s) | volume {:.2} | shuffle {} | repeat {}",
            state,
            self.dir.display(),
            self.names.len(),
            self.volume,
            if self.shuffle { "on" } else { "off" },
            match self.repeat {
                Repeat::All => "all",
                Repeat::Off => "off",
                Repeat::One => "one",
            },
        );
        if !self.status.is_empty() {
            line.push('\n');
            line.push_str(&self.status);
        }
        line
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<MusicPlayer>()
        .add_systems(Startup, scan_music_on_start)
        .add_systems(Update, drive_music.in_set(frame::ClientSet::Effects));
}

fn music_dir() -> PathBuf {
    match std::env::var_os("IW4L_MUSIC") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => std::env::current_dir()
            .map(|dir| dir.join("music"))
            .unwrap_or_else(|_| PathBuf::from("music")),
    }
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let ext = ext.to_ascii_lowercase();
            EXTENSIONS.contains(&ext.as_str())
        })
}

fn scan_music_on_start(mut player: ResMut<MusicPlayer>) {
    player.dir = music_dir();
    scan(&mut player);
}

/// (Re)read the folder. Sorted so a fixed order is stable run to run.
fn scan(player: &mut MusicPlayer) {
    player.paths.clear();
    player.names.clear();
    if let Err(error) = std::fs::create_dir_all(&player.dir) {
        player.status = format!("music: cannot create {}: {error}", player.dir.display());
        return;
    }
    let entries = match std::fs::read_dir(&player.dir) {
        Ok(entries) => entries,
        Err(error) => {
            player.status = format!("music: cannot read {}: {error}", player.dir.display());
            return;
        }
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_supported(path))
        .collect();
    found.sort();
    for path in found {
        player.names.push(
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("?")
                .to_owned(),
        );
        player.paths.push(path);
    }
    player.scanned = true;
    player.status = if player.names.is_empty() {
        format!(
            "music: no tracks in {} — drop .mp3/.wav/.ogg/.flac files there",
            player.dir.display()
        )
    } else {
        format!("music: {} track(s) in {}", player.names.len(), player.dir.display())
    };
    diag::info!(Audio, "{}", player.status);
}

fn next_rng(rng: &mut u64) -> u64 {
    // xorshift64*
    let mut x = *rng;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *rng = x;
    x.wrapping_mul(0x2545_f491_4f6c_dd1d)
}

fn pick_next(player: &mut MusicPlayer, forward: bool) -> Option<usize> {
    let count = player.paths.len();
    if count == 0 {
        return None;
    }
    if player.shuffle && count > 1 {
        let mut pick = (next_rng(&mut player.rng) % count as u64) as usize;
        if pick == player.index {
            pick = (pick + 1) % count;
        }
        return Some(pick);
    }
    if forward {
        let next = player.index + 1;
        if next < count {
            Some(next)
        } else if player.repeat == Repeat::All {
            Some(0)
        } else {
            None
        }
    } else {
        Some(player.index.checked_sub(1).unwrap_or(count - 1))
    }
}

fn stop_current(player: &mut MusicPlayer, commands: &mut Commands) {
    if let Some(entity) = player.current.take() {
        commands.entity(entity).try_despawn();
    }
    player.current_started = false;
    player.current_elapsed = 0.0;
}

fn start(
    player: &mut MusicPlayer,
    commands: &mut Commands,
    assets: &mut Assets<PcmAudio>,
    index: usize,
) {
    let Some(path) = player.paths.get(index).cloned() else {
        return;
    };
    stop_current(player, commands);
    player.index = index;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            player.status = format!("music: cannot read {}: {error}", path.display());
            player.enabled = false;
            diag::warn!(Audio, "{}", player.status);
            return;
        }
    };
    // Music files are typically mastered well below the game's effects, so
    // peak-normalise each track before it enters the mixer. The console volume
    // then scales that.
    let Some(pcm) = decode_audio_bytes(&bytes).map(|pcm| pcm.normalized(0.95)) else {
        player.status = format!("music: cannot decode {} (unsupported codec)", path.display());
        player.enabled = false;
        diag::warn!(Audio, "{}", player.status);
        return;
    };
    let handle = assets.add(pcm);
    let entity = commands
        .spawn((
            AudioPlayer(handle),
            PlaybackSettings::ONCE.with_volume(Volume::Linear(player.volume)),
            MusicTrack,
        ))
        .id();
    player.current = Some(entity);
    player.current_started = false;
    player.current_elapsed = 0.0;
    player.applied_volume = player.volume;
    let name = player.track_name(index).unwrap_or("?").to_owned();
    player.status = format!("music: playing `{name}`");
    diag::info!(Audio, "{}", player.status);
}

/// Called when a track ends; moves on (or stops) per repeat/shuffle.
fn advance(player: &mut MusicPlayer, commands: &mut Commands, assets: &mut Assets<PcmAudio>) {
    if player.repeat == Repeat::One {
        let index = player.index;
        start(player, commands, assets, index);
        return;
    }
    match pick_next(player, true) {
        Some(index) => start(player, commands, assets, index),
        None => {
            stop_current(player, commands);
            player.enabled = false;
            player.status = "music: end of playlist".to_owned();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_music(
    mut player: ResMut<MusicPlayer>,
    mut commands: Commands,
    mut assets: ResMut<Assets<PcmAudio>>,
    time: Res<Time>,
    mut sinks: Query<&mut AudioSink, With<MusicTrack>>,
) {
    if let Some(request) = player.request.take() {
        match request {
            MusicRequest::On => {
                player.enabled = true;
                if player.current.is_none() {
                    let index = player.index;
                    start(&mut player, &mut commands, &mut assets, index);
                }
            }
            MusicRequest::Off => {
                player.enabled = false;
                stop_current(&mut player, &mut commands);
                player.status = "music: stopped".to_owned();
            }
            MusicRequest::Toggle => {
                if player.enabled {
                    player.enabled = false;
                    stop_current(&mut player, &mut commands);
                    player.status = "music: stopped".to_owned();
                } else {
                    player.enabled = true;
                    if player.current.is_none() {
                        let index = player.index;
                        start(&mut player, &mut commands, &mut assets, index);
                    }
                }
            }
            MusicRequest::Next => match pick_next(&mut player, true) {
                Some(index) => {
                    player.enabled = true;
                    start(&mut player, &mut commands, &mut assets, index);
                }
                None => {
                    player.status = "music: no next track".to_owned();
                }
            },
            MusicRequest::Prev => {
                if let Some(index) = pick_next(&mut player, false) {
                    player.enabled = true;
                    start(&mut player, &mut commands, &mut assets, index);
                }
            }
            MusicRequest::Track(index) => {
                if index < player.paths.len() {
                    player.enabled = true;
                    start(&mut player, &mut commands, &mut assets, index);
                } else {
                    player.status = format!("music: no track {}", index + 1);
                }
            }
            MusicRequest::Reload => {
                scan(&mut player);
                if player.enabled {
                    let index = player.index.min(player.paths.len().saturating_sub(1));
                    if !player.paths.is_empty() {
                        start(&mut player, &mut commands, &mut assets, index);
                    }
                }
            }
        }
    }

    let Some(entity) = player.current else {
        return;
    };
    player.current_elapsed += time.delta_secs();
    let live = sinks.get_mut(entity);
    let finished = match &live {
        Ok(sink) => {
            player.current_started = true;
            sink.empty()
        }
        // The sink is spawned a frame or two after the player; only treat a
        // missing entity as "finished" once it had started (Bevy despawns
        // finished one-shot players).
        Err(_) => player.current_started || player.current_elapsed > START_TIMEOUT_SECS,
    };

    if let Ok(mut sink) = live
        && (player.volume - player.applied_volume).abs() > 1e-3
    {
        sink.set_volume(Volume::Linear(player.volume.clamp(0.0, 1.0)));
        player.applied_volume = player.volume;
    }

    if finished && player.enabled {
        player.current = None;
        advance(&mut player, &mut commands, &mut assets);
    } else if finished {
        player.current = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_extensions_are_case_insensitive() {
        assert!(is_supported(Path::new("a/b/Track 1.mp3")));
        assert!(is_supported(Path::new("song.WAV")));
        assert!(is_supported(Path::new("song.flac")));
        assert!(!is_supported(Path::new("song.mus")));
        assert!(!is_supported(Path::new("song")));
    }

    fn player_with(count: usize) -> MusicPlayer {
        let mut player = MusicPlayer::default();
        for index in 0..count {
            player.names.push(format!("track{index}"));
            player.paths.push(PathBuf::from(format!("track{index}.mp3")));
        }
        player
    }

    #[test]
    fn sequential_next_wraps_only_when_repeating() {
        let mut player = player_with(3);
        player.index = 2;
        player.repeat = Repeat::Off;
        assert_eq!(pick_next(&mut player, true), None);
        player.repeat = Repeat::All;
        assert_eq!(pick_next(&mut player, true), Some(0));
        player.index = 1;
        assert_eq!(pick_next(&mut player, true), Some(2));
    }

    #[test]
    fn prev_wraps_to_the_last_track() {
        let mut player = player_with(3);
        player.index = 0;
        assert_eq!(pick_next(&mut player, false), Some(2));
    }

    #[test]
    fn shuffle_never_picks_the_current_track() {
        let mut player = player_with(4);
        player.shuffle = true;
        player.index = 1;
        for _ in 0..32 {
            let next = pick_next(&mut player, true).unwrap();
            assert_ne!(next, 1);
        }
    }
}

