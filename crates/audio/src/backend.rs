use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use bevy::{
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume},
    prelude::*,
};
use frame::ClientSet;

use crate::pcm::{LoopingPcmAudio, LoopingPcmPlayback, PcmAudio};
use crate::voice::reclaim_finished_voices;

const STARTING_TIMEOUT: Duration = Duration::from_millis(250);

/// Game-sound gain, applied to every voice the backend starts. Kept in an
/// atomic so the spawn helpers — which are not systems and are called from a
/// dozen places — see the latest value without threading `GameSettings`
/// through every signature. Music has its own player and does not read this.
static MASTER_GAIN: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0f32 bits

pub(crate) fn set_master_gain(value: f32) {
    MASTER_GAIN.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}

fn master_gain() -> f32 {
    f32::from_bits(MASTER_GAIN.load(Ordering::Relaxed))
}

fn game_volume(gain: f32) -> Volume {
    Volume::Linear((gain * master_gain()).max(0.0))
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchEpoch(pub u64);

impl MatchEpoch {
    pub fn bump(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioScope {
    Menu,
    Match,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceKind {
    Oneshot,
    Loop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceOwner {
    Exclusive,

    Attached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoicePhase {
    Starting,
    Playing,
}

#[derive(Component, Debug)]
pub struct Voice {
    pub epoch: u64,
    pub scope: AudioScope,
    kind: VoiceKind,
    owner: VoiceOwner,
    phase: VoicePhase,
    started_at: Instant,
}

/// Last game-sound gain that was pushed to the sinks.
#[derive(Resource)]
struct AppliedGain(f32);

impl Default for AppliedGain {
    fn default() -> Self {
        Self(1.0)
    }
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<MatchEpoch>()
        .init_resource::<AppliedGain>()
        .add_systems(Update, sync_master_gain.in_set(ClientSet::Effects))
        .add_systems(
            Update,
            (cancel_stale_match_voices, advance_voice_phases)
                .chain()
                .before(reclaim_finished_voices)
                .in_set(ClientSet::Effects),
        );
}

/// Mirror the game's sound setting into the atomic the spawn helpers read, and
/// rescale the game voices that do not recompute their volume every frame
/// (positional and ambient sinks do, so they are excluded to avoid applying the
/// change twice). Music keeps its own player and is left alone.
fn sync_master_gain(
    settings: Res<frame::GameSettings>,
    mut applied: ResMut<AppliedGain>,
    mut voices: Query<
        (&mut AudioSink, &PlaybackSettings),
        (
            With<Voice>,
            Without<crate::music::MusicTrack>,
            Without<crate::playback::Channel3d>,
            Without<crate::ambient::MapEmitter>,
        ),
    >,
) {
    let next = settings.master_volume.clamp(0.0, 1.0);
    set_master_gain(next);
    if (next - applied.0).abs() < 1e-4 {
        return;
    }
    let previous = applied.0;
    for (mut sink, playback) in &mut voices {
        let base = if previous > f32::EPSILON {
            sink.volume().to_linear() / previous
        } else {
            playback.volume.to_linear()
        };
        sink.set_volume(Volume::Linear(base * next));
    }
    applied.0 = next;
}

pub(crate) fn spawn_oneshot(
    commands: &mut Commands,
    handle: Handle<PcmAudio>,
    gain: f32,
    speed: f32,
    epoch: u64,
    scope: AudioScope,
) -> Entity {
    commands
        .spawn((
            AudioPlayer(handle),
            PlaybackSettings::ONCE
                .with_volume(game_volume(gain))
                .with_speed(speed),
            Voice {
                epoch,
                scope,
                kind: VoiceKind::Oneshot,
                owner: VoiceOwner::Exclusive,
                phase: VoicePhase::Starting,
                started_at: Instant::now(),
            },
        ))
        .id()
}

pub(crate) fn spawn_loop(
    commands: &mut Commands,
    handle: Handle<LoopingPcmAudio>,
    gain: f32,
    epoch: u64,
    scope: AudioScope,
) -> Entity {
    commands
        .spawn((
            LoopingPcmPlayback::new(handle, game_volume(gain)),
            Voice {
                epoch,
                scope,
                kind: VoiceKind::Loop,
                owner: VoiceOwner::Exclusive,
                phase: VoicePhase::Starting,
                started_at: Instant::now(),
            },
        ))
        .id()
}

pub(crate) fn attach_loop(
    commands: &mut Commands,
    entity: Entity,
    handle: Handle<LoopingPcmAudio>,
    gain: f32,
    epoch: u64,
) {
    commands.entity(entity).insert((
        LoopingPcmPlayback::new(handle, game_volume(gain)),
        Voice {
            epoch,
            scope: AudioScope::Match,
            kind: VoiceKind::Loop,
            owner: VoiceOwner::Attached,
            phase: VoicePhase::Starting,
            started_at: Instant::now(),
        },
    ));
}

pub(crate) fn detach_loop(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).remove::<(
        AudioPlayer<LoopingPcmAudio>,
        PlaybackSettings,
        AudioSink,
        Voice,
    )>();
}

pub(crate) fn stop(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).try_despawn();
}

fn advance_voice_phases(
    mut voices: Query<(Entity, &mut Voice, Option<&AudioSink>)>,
    mut commands: Commands,
) {
    let now = Instant::now();
    for (entity, mut voice, sink) in &mut voices {
        match voice.phase {
            VoicePhase::Starting => {
                if sink.is_some() {
                    voice.phase = VoicePhase::Playing;
                    continue;
                }
                if now.duration_since(voice.started_at) >= STARTING_TIMEOUT {
                    diag::warn!(
                        Audio,
                        "audio: voice start timed out waiting for sink (typed gap)"
                    );
                    end_voice(&mut commands, entity, voice.owner);
                }
            }
            VoicePhase::Playing => {
                if voice.kind != VoiceKind::Oneshot {
                    continue;
                }
                if sink.is_some_and(|s| s.empty()) {
                    end_voice(&mut commands, entity, voice.owner);
                }
            }
        }
    }
}

fn cancel_stale_match_voices(
    epoch: Res<MatchEpoch>,
    voices: Query<(Entity, &Voice)>,
    mut commands: Commands,
) {
    for (entity, voice) in &voices {
        if voice.scope != AudioScope::Match {
            continue;
        }
        if voice.epoch == epoch.0 {
            continue;
        }
        end_voice(&mut commands, entity, voice.owner);
    }
}

fn end_voice(commands: &mut Commands, entity: Entity, owner: VoiceOwner) {
    match owner {
        VoiceOwner::Exclusive => {
            commands.entity(entity).try_despawn();
        }
        VoiceOwner::Attached => {
            detach_loop(commands, entity);
        }
    }
}
