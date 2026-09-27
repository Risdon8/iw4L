//! Mod: `wallrun` — toggle and tune Titanfall-style wall-running.
//!
//! The values live in the sim (`MovementTuning`) and change only through
//! `ClientAction::SetMovementTuning`, so prediction and replay agree with the
//! authority.

use bevy::prelude::*;
use net::{ClientActionInbox, LocalPresentClient};
use sim::movement_tuning::{
    WALLRUN_DEFAULT_COOLDOWN_MS, WALLRUN_DEFAULT_JUMP_OUT, WALLRUN_DEFAULT_JUMP_UP,
    WALLRUN_DEFAULT_MIN_SPEED, WALLRUN_DEFAULT_TIME_MS,
};
use sim::{ClientAction, MovementTuning};

use crate::{ConsoleCommand, ConsoleLine, ConsoleRegistry, ConsoleSettings, ConsoleState};

const USAGE: &str = "wallrun [on|off|reset] | wallrun time <ms> | cooldown <ms> | speed <n> | up <n> | out <n> — wall-running (needs cheats; mod)";

pub(crate) fn register_wallrun_commands(registry: &mut ConsoleRegistry) {
    if registry.resolve("wallrun").is_none() {
        registry.register(crate::CommandSpec::new("wallrun").usage(USAGE).arg(
            crate::StaticCompleter::new([
                "on", "off", "reset", "time", "cooldown", "speed", "up", "out",
            ]),
        ));
    }
}

/// `pending` is the last tuning queued but not yet seen in the authority world,
/// so `wallrun on; wallrun time 3000` in one frame builds on the first edit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn route_wallrun_commands(
    mut events: MessageReader<ConsoleCommand>,
    mut console: ResMut<ConsoleState>,
    settings: Res<ConsoleSettings>,
    mut line: ResMut<ConsoleLine>,
    local: Res<LocalPresentClient>,
    authority: Option<Res<net::AuthorityWorld>>,
    mut inbox: Option<ResMut<ClientActionInbox>>,
    mut seq: ResMut<net::ActionRequestIds>,
    mut pending: Local<Option<MovementTuning>>,
) {
    let capacity = settings.log_capacity;
    let echo = |msg: String, console: &mut ConsoleState, line: &mut ConsoleLine| {
        diag::info!(Console, "{msg}");
        line.0 = msg.clone();
        console.echo(msg, capacity);
    };

    let applied = authority.as_ref().map(|a| a.0.movement_tuning());
    if pending.is_some() && *pending == applied {
        *pending = None;
    }

    for cmd in events.read() {
        if cmd.name != "wallrun" {
            continue;
        }
        let Some(applied) = applied else {
            echo(
                "wallrun: no authority world (not a listen host)".into(),
                &mut console,
                &mut line,
            );
            continue;
        };
        let base = pending.unwrap_or(applied);
        let next = match parse_wallrun(&cmd.args, base) {
            Ok(None) => {
                echo(format_tuning("wallrun", &base), &mut console, &mut line);
                continue;
            }
            Ok(Some(next)) => next,
            Err(msg) => {
                echo(msg, &mut console, &mut line);
                continue;
            }
        };
        if authority.as_ref().is_some_and(|a| !a.0.cheats_enabled()) {
            echo("wallrun: cheats are off".into(), &mut console, &mut line);
            continue;
        }
        let Some(inbox) = inbox.as_deref_mut() else {
            echo(
                "wallrun: no action inbox (not a listen host)".into(),
                &mut console,
                &mut line,
            );
            continue;
        };
        let request_id = seq.allocate();
        if let Err(error) = inbox.push(
            local.0,
            ClientAction::SetMovementTuning {
                request_id,
                tuning: next,
            },
        ) {
            echo(format!("wallrun: {error}"), &mut console, &mut line);
            continue;
        }
        *pending = Some(next.sanitized());
        echo(
            format!(
                "{} request_id={request_id}",
                format_tuning("wallrun: queued", &next)
            ),
            &mut console,
            &mut line,
        );
    }
}

/// `Ok(None)` is a status query.
fn parse_wallrun(args: &[String], base: MovementTuning) -> Result<Option<MovementTuning>, String> {
    let mut next = base;
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => return Ok(None),
        ["on" | "1"] => next.wallrun = true,
        ["off" | "0"] => next.wallrun = false,
        ["reset"] => {
            next.wallrun_time_ms = WALLRUN_DEFAULT_TIME_MS;
            next.wallrun_cooldown_ms = WALLRUN_DEFAULT_COOLDOWN_MS;
            next.wallrun_min_speed = WALLRUN_DEFAULT_MIN_SPEED;
            next.wallrun_jump_up = WALLRUN_DEFAULT_JUMP_UP;
            next.wallrun_jump_out = WALLRUN_DEFAULT_JUMP_OUT;
        }
        ["time", value] => next.wallrun_time_ms = parse_millis("time", value)?,
        ["cooldown", value] => next.wallrun_cooldown_ms = parse_millis("cooldown", value)?,
        ["speed", value] => next.wallrun_min_speed = parse_non_negative("speed", value)?,
        ["up", value] => next.wallrun_jump_up = parse_non_negative("up", value)?,
        ["out", value] => next.wallrun_jump_out = parse_non_negative("out", value)?,
        _ => return Err(format!("usage: {USAGE}")),
    }
    Ok(Some(next))
}

fn parse_non_negative(name: &str, raw: &str) -> Result<f32, String> {
    match raw.parse::<f32>() {
        Ok(value) if value.is_finite() && value >= 0.0 => Ok(value),
        _ => Err(format!(
            "wallrun {name}: expected a non-negative number, got `{raw}`"
        )),
    }
}

fn parse_millis(name: &str, raw: &str) -> Result<i32, String> {
    match raw.parse::<i32>() {
        Ok(value) if value >= 0 => Ok(value),
        _ => Err(format!(
            "wallrun {name}: expected a non-negative whole number of ms, got `{raw}`"
        )),
    }
}

fn format_tuning(prefix: &str, tuning: &MovementTuning) -> String {
    format!(
        "{prefix} {} time={} cooldown={} speed={} up={} out={}",
        if tuning.wallrun { "on" } else { "off" },
        tuning.wallrun_time_ms,
        tuning.wallrun_cooldown_ms,
        tuning.wallrun_min_speed,
        tuning.wallrun_jump_up,
        tuning.wallrun_jump_out,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn bare_is_status_query() {
        assert_eq!(parse_wallrun(&[], MovementTuning::default()), Ok(None));
    }

    #[test]
    fn on_keeps_tuning_values() {
        let base = MovementTuning {
            wallrun_time_ms: 3000,
            ..MovementTuning::default()
        };
        let next = parse_wallrun(&args(&["on"]), base).unwrap().unwrap();
        assert!(next.wallrun);
        assert_eq!(next.wallrun_time_ms, 3000);
    }

    #[test]
    fn reset_restores_defaults_without_toggling() {
        let base = MovementTuning {
            wallrun: true,
            wallrun_time_ms: 999,
            ..MovementTuning::default()
        };
        let next = parse_wallrun(&args(&["reset"]), base).unwrap().unwrap();
        assert!(next.wallrun);
        assert_eq!(next.wallrun_time_ms, WALLRUN_DEFAULT_TIME_MS);
    }

    #[test]
    fn rejects_negative_and_garbage() {
        let base = MovementTuning::default();
        assert!(parse_wallrun(&args(&["time", "-1"]), base).is_err());
        assert!(parse_wallrun(&args(&["speed", "fast"]), base).is_err());
        assert!(parse_wallrun(&args(&["sideways"]), base).is_err());
    }
}
