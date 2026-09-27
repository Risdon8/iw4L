//! Mod: `surf` — toggle and tune Source-style air acceleration.
//!
//! The value lives in the sim (`MovementTuning`) and is changed only through
//! `ClientAction::SetMovementTuning`, so prediction and replay agree with the
//! authority.

use bevy::prelude::*;
use net::{ClientActionInbox, LocalPresentClient};
use sim::movement_tuning::{SURF_DEFAULT_AIR_ACCEL, SURF_DEFAULT_AIR_WISHSPEED_CAP};
use sim::{ClientAction, MovementTuning};

use crate::{ConsoleCommand, ConsoleLine, ConsoleRegistry, ConsoleSettings, ConsoleState};

const USAGE: &str =
    "surf [on|off|reset] | surf accel <n> | surf cap <n> — Source air strafing (needs cheats; mod)";

pub(crate) fn register_surf_commands(registry: &mut ConsoleRegistry) {
    if registry.resolve("surf").is_none() {
        registry.register(crate::CommandSpec::new("surf").usage(USAGE).arg(
            crate::StaticCompleter::new(["on", "off", "reset", "accel", "cap"]),
        ));
    }
}

/// `pending` is the last tuning queued but not yet seen in the authority
/// world, so `surf on; surf accel 150` in one frame builds on the first edit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn route_surf_commands(
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
        if cmd.name != "surf" {
            continue;
        }
        let Some(applied) = applied else {
            echo(
                "surf: no authority world (not a listen host)".into(),
                &mut console,
                &mut line,
            );
            continue;
        };
        let base = pending.unwrap_or(applied);
        let next = match parse_surf(&cmd.args, base) {
            Ok(None) => {
                echo(format_tuning("surf", &base), &mut console, &mut line);
                continue;
            }
            Ok(Some(next)) => next,
            Err(msg) => {
                echo(msg, &mut console, &mut line);
                continue;
            }
        };
        if authority.as_ref().is_some_and(|a| !a.0.cheats_enabled()) {
            echo("surf: cheats are off".into(), &mut console, &mut line);
            continue;
        }
        let Some(inbox) = inbox.as_deref_mut() else {
            echo(
                "surf: no action inbox (not a listen host)".into(),
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
            echo(format!("surf: {error}"), &mut console, &mut line);
            continue;
        }
        *pending = Some(next.sanitized());
        echo(
            format!(
                "{} request_id={request_id}",
                format_tuning("surf: queued", &next)
            ),
            &mut console,
            &mut line,
        );
    }
}

/// `Ok(None)` is a status query.
fn parse_surf(args: &[String], base: MovementTuning) -> Result<Option<MovementTuning>, String> {
    let mut next = base;
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => return Ok(None),
        ["on" | "1"] => next.surf = true,
        ["off" | "0"] => next.surf = false,
        ["reset"] => {
            next = MovementTuning {
                surf: base.surf,
                surf_air_accel: SURF_DEFAULT_AIR_ACCEL,
                surf_air_wishspeed_cap: SURF_DEFAULT_AIR_WISHSPEED_CAP,
            };
        }
        ["accel", value] => next.surf_air_accel = parse_non_negative("accel", value)?,
        ["cap", value] => next.surf_air_wishspeed_cap = parse_non_negative("cap", value)?,
        _ => return Err(format!("usage: {USAGE}")),
    }
    Ok(Some(next))
}

fn parse_non_negative(name: &str, raw: &str) -> Result<f32, String> {
    match raw.parse::<f32>() {
        Ok(value) if value.is_finite() && value >= 0.0 => Ok(value),
        _ => Err(format!(
            "surf {name}: expected a non-negative number, got `{raw}`"
        )),
    }
}

fn format_tuning(prefix: &str, tuning: &MovementTuning) -> String {
    format!(
        "{prefix} {} accel={} cap={}",
        if tuning.surf { "on" } else { "off" },
        tuning.surf_air_accel,
        tuning.surf_air_wishspeed_cap,
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
        assert_eq!(parse_surf(&[], MovementTuning::default()), Ok(None));
    }

    #[test]
    fn on_keeps_tuning_values() {
        let base = MovementTuning {
            surf_air_accel: 150.0,
            ..MovementTuning::default()
        };
        let next = parse_surf(&args(&["on"]), base).unwrap().unwrap();
        assert!(next.surf);
        assert_eq!(next.surf_air_accel, 150.0);
    }

    #[test]
    fn accel_keeps_toggle() {
        let base = MovementTuning {
            surf: true,
            ..MovementTuning::default()
        };
        let next = parse_surf(&args(&["accel", "150"]), base).unwrap().unwrap();
        assert!(next.surf);
        assert_eq!(next.surf_air_accel, 150.0);
    }

    #[test]
    fn rejects_negative_and_garbage() {
        let base = MovementTuning::default();
        assert!(parse_surf(&args(&["cap", "-1"]), base).is_err());
        assert!(parse_surf(&args(&["accel", "fast"]), base).is_err());
        assert!(parse_surf(&args(&["sideways"]), base).is_err());
    }
}
