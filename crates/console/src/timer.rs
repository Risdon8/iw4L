//! Mod: `timer` — a run timer with checkpoint splits.
//!
//! The timer follows the sim's layout checkpoint progress: it starts when the
//! player first enters a course checkpoint, records a split at each new one and
//! stops at the last. `timer reset` cancels the current run; `timer clear`
//! forgets the best. The speedometer is drawn by the HUD from the snapshot.

use bevy::prelude::*;
use frame::{RunSplit, RunTimer};

use crate::{ConsoleCommand, ConsoleLine, ConsoleRegistry, ConsoleSettings, ConsoleState};

const USAGE: &str = "timer | timer start|stop|reset|clear — run timer with checkpoint splits (mod)";

pub(crate) fn register_timer_commands(registry: &mut ConsoleRegistry) {
    if registry.resolve("timer").is_none() {
        registry.register(
            crate::CommandSpec::new("timer")
                .usage(USAGE)
                .arg(crate::StaticCompleter::new([
                    "start", "stop", "reset", "clear",
                ])),
        );
    }
}

fn clock(seconds: f32) -> String {
    let cs = (seconds.max(0.0) * 100.0).round() as i64;
    format!("{}:{:02}.{:02}", cs / 6000, (cs / 100) % 60, cs % 100)
}

#[derive(Default)]
pub(crate) struct RunClock {
    progress: Option<usize>,
    best_splits: Vec<Option<f32>>,
    course: String,
}

fn cancel(timer: &mut RunTimer, state: &mut RunClock) {
    timer.running = false;
    timer.elapsed = 0.0;
    timer.splits.clear();
    timer.last_split = None;
    state.progress = None;
    timer.status = "run reset".to_owned();
}

fn summary(timer: &RunTimer) -> String {
    let best = timer
        .best
        .map(clock)
        .unwrap_or_else(|| "--:--.--".to_owned());
    let mut line = format!(
        "timer course=`{}` checkpoints={} running={} time={} best={}",
        timer.course,
        timer.checkpoint_count,
        timer.running,
        clock(timer.elapsed),
        best
    );
    if let Some(label) = timer.next_label.as_deref() {
        line.push_str(&format!(
            " next=`{label}` {:.0}u {}",
            timer.next_distance.unwrap_or(0.0),
            timer.next_dir
        ));
    }
    if !timer.splits.is_empty() {
        line.push('\n');
        for split in &timer.splits {
            let delta = split
                .delta
                .map(|d| format!("{d:+.2}"))
                .unwrap_or_else(|| "  -   ".to_owned());
            line.push_str(&format!("\n  {:<16} {}  {delta}", split.label, clock(split.time)));
        }
    }
    line
}

pub(crate) fn route_timer_commands(
    mut events: MessageReader<ConsoleCommand>,
    mut console: ResMut<ConsoleState>,
    settings: Res<ConsoleSettings>,
    mut line: ResMut<ConsoleLine>,
    mut timer: ResMut<RunTimer>,
    mut state: Local<RunClock>,
) {
    let capacity = settings.log_capacity;
    let echo = |msg: String, console: &mut ConsoleState, line: &mut ConsoleLine| {
        diag::info!(Console, "{msg}");
        line.0 = msg.clone();
        console.echo(msg, capacity);
    };

    for cmd in events.read() {
        if cmd.name != "timer" {
            continue;
        }
        match cmd.args.first().map(String::as_str) {
            None | Some("status") => echo(summary(&timer), &mut console, &mut line),
            Some("start") => {
                timer.running = true;
                timer.elapsed = 0.0;
                timer.splits.clear();
                timer.last_split = None;
                state.progress = None;
                timer.status = "run started".to_owned();
                echo("timer: started".to_owned(), &mut console, &mut line);
            }
            Some("stop") => {
                timer.running = false;
                echo("timer: stopped".to_owned(), &mut console, &mut line);
            }
            Some("reset") => {
                cancel(&mut timer, &mut state);
                echo("timer: reset".to_owned(), &mut console, &mut line);
            }
            Some("clear") => {
                timer.best = None;
                timer.splits.clear();
                timer.last_split = None;
                state.best_splits.clear();
                state.progress = None;
                echo("timer: best cleared".to_owned(), &mut console, &mut line);
            }
            Some(other) => echo(
                format!("timer: unknown `{other}`\n{USAGE}"),
                &mut console,
                &mut line,
            ),
        }
    }
}

pub(crate) fn update_run_timer(
    authority: Option<Res<net::AuthorityWorld>>,
    local: Res<net::LocalPresentClient>,
    presented: Res<net::PresentedSnapshot>,
    time: Res<Time>,
    mut timer: ResMut<RunTimer>,
    mut state: Local<RunClock>,
) {
    let Some(authority) = authority else {
        return;
    };
    let (course, labels, origins): (String, Vec<String>, Vec<[f32; 3]>) = match map_layout::active() {
        Some(active) => (
            active.name.clone(),
            active
                .checkpoints
                .iter()
                .enumerate()
                .map(|(index, checkpoint)| {
                    if checkpoint.name.is_empty() {
                        format!("#{index}")
                    } else {
                        checkpoint.name.clone()
                    }
                })
                .collect(),
            active.checkpoints.iter().map(|checkpoint| checkpoint.origin).collect(),
        ),
        None => (String::new(), Vec::new(), Vec::new()),
    };
    let count = labels.len();

    if course != state.course {
        state.course = course.clone();
        state.best_splits = vec![None; count];
        state.progress = None;
        *timer = RunTimer::default();
        timer.course = course;
        timer.checkpoint_count = count;
        timer.status = if count == 0 {
            "no course checkpoints".to_owned()
        } else {
            format!("course `{}` — {count} checkpoints", state.course)
        };
        return;
    }
    timer.course = course;
    timer.checkpoint_count = count;

    let alive = presented.alive_player(local.0).is_some();
    if !alive {
        if timer.running {
            cancel(&mut timer, &mut state);
        }
        return;
    }
    let index = authority.0.layout_checkpoint(local.0);
    if count == 0 {
        return;
    }

    // Waypoint to the next checkpoint (the start when idle).
    timer.next_label = None;
    timer.next_distance = None;
    timer.next_dir.clear();
    let target = if timer.running {
        state.progress.map_or(0, |progress| progress + 1)
    } else {
        0
    };
    if target < count
        && let Some(player) = presented.alive_player(local.0)
        && let Some(origin) = origins.get(target)
    {
        let dx = origin[0] - player.origin[0];
        let dy = origin[1] - player.origin[1];
        let rel = ((dy.atan2(dx).to_degrees() - player.viewangles[1] + 540.0) % 360.0) - 180.0;
        timer.next_distance = Some((dx * dx + dy * dy).sqrt());
        timer.next_dir = if rel.abs() < 45.0 {
            "AHEAD"
        } else if rel.abs() > 135.0 {
            "BEHIND"
        } else if rel > 0.0 {
            "LEFT"
        } else {
            "RIGHT"
        }
        .to_owned();
        timer.next_label = labels.get(target).cloned();
    }

    if !timer.running {
        // Start the moment the player is in a checkpoint and idle-timed.
        if index.is_some() && timer.elapsed == 0.0 {
            timer.running = true;
            timer.status = "run started".to_owned();
        } else {
            return;
        }
    }
    timer.elapsed += time.delta_secs();

    match index {
        None => cancel(&mut timer, &mut state),
        Some(index) if state.progress != Some(index) => {
            state.progress = Some(index);
            let label = labels.get(index).cloned().unwrap_or_else(|| format!("#{index}"));
            let best = state.best_splits.get(index).copied().flatten();
            let split = RunSplit {
                label,
                time: timer.elapsed,
                delta: best.map(|best| timer.elapsed - best),
            };
            timer.splits.push(split.clone());
            timer.last_split = Some(split);
            if count > 1 && index + 1 == count {
                timer.running = false;
                let finished = timer.elapsed;
                if timer.best.is_none_or(|best| finished < best) {
                    timer.best = Some(finished);
                }
                for (slot_index, split) in timer.splits.iter().enumerate() {
                    if slot_index < state.best_splits.len() {
                        let slot = &mut state.best_splits[slot_index];
                        *slot = Some(slot.map_or(split.time, |best| best.min(split.time)));
                    }
                }
                timer.status = format!("finished {}", clock(finished));
            } else {
                timer.status = format!("split {}/{}", index + 1, count);
            }
        }
        Some(_) => {}
    }
}
