//! Mod: `movement` — the global movement profile.
//!
//! Applied on every map at match start (`MovementTuning::fluid`), so a real map
//! needs no layout. This command shows and switches it live; the `surf` and
//! `wallrun` commands tune the finer numbers.

use bevy::prelude::*;
use net::{ClientActionInbox, LocalPresentClient};
use sim::{ClientAction, MovementTuning};

use crate::{ConsoleCommand, ConsoleLine, ConsoleRegistry, ConsoleSettings, ConsoleState};

const USAGE: &str = "movement | movement fluid|retail | movement surf|wallrun|doublejump|slide on|off — global movement profile (needs cheats; mod)";

pub(crate) fn register_movement_commands(registry: &mut ConsoleRegistry) {
    if registry.resolve("movement").is_none() {
        registry.register(crate::CommandSpec::new("movement").usage(USAGE).arg(
            crate::StaticCompleter::new([
                "fluid",
                "retail",
                "surf",
                "wallrun",
                "doublejump",
                "slide",
            ]),
        ));
    }
    if registry.resolve("doublejump").is_none() {
        registry.register(
            crate::CommandSpec::new("doublejump")
                .usage("doublejump [on|off] — one extra jump in the air (mod)")
                .arg(crate::StaticCompleter::new(["on", "off"])),
        );
    }
    if registry.resolve("slide").is_none() {
        registry.register(
            crate::CommandSpec::new("slide")
                .usage("slide [on|off] — crouch/power slide (mod)")
                .arg(crate::StaticCompleter::new(["on", "off"])),
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn route_movement_commands(
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
        let name = cmd.name.clone();
        if name != "movement" && name != "doublejump" && name != "slide" {
            continue;
        }
        let Some(applied) = applied else {
            echo(
                format!("{name}: no authority world (not a listen host)"),
                &mut console,
                &mut line,
            );
            continue;
        };
        let base = pending.unwrap_or(applied);
        let parsed = match name.as_str() {
            "doublejump" => parse_double_jump(&cmd.args, base),
            "slide" => parse_slide(&cmd.args, base),
            _ => parse_movement(&cmd.args, base),
        };
        let next = match parsed {
            Ok(None) => {
                echo(format_profile(&name, &base), &mut console, &mut line);
                continue;
            }
            Ok(Some(next)) => next,
            Err(msg) => {
                echo(msg, &mut console, &mut line);
                continue;
            }
        };
        if authority.as_ref().is_some_and(|a| !a.0.cheats_enabled()) {
            echo(format!("{name}: cheats are off"), &mut console, &mut line);
            continue;
        }
        let Some(inbox) = inbox.as_deref_mut() else {
            echo(
                format!("{name}: no action inbox (not a listen host)"),
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
            echo(format!("{name}: {error}"), &mut console, &mut line);
            continue;
        }
        *pending = Some(next.sanitized());
        echo(
            format!(
                "{} request_id={request_id}",
                format_profile(&format!("{name}: queued"), &next)
            ),
            &mut console,
            &mut line,
        );
    }
}

/// `Ok(None)` is a status query.
fn parse_movement(args: &[String], base: MovementTuning) -> Result<Option<MovementTuning>, String> {
    let mut next = base;
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => return Ok(None),
        ["fluid"] => {
            next.surf = true;
            next.wallrun = true;
            next.double_jump = true;
            next.slide = true;
        }
        ["retail" | "off"] => {
            next.surf = false;
            next.wallrun = false;
            next.double_jump = false;
            next.slide = false;
        }
        ["surf", value] => next.surf = parse_bool("surf", value)?,
        ["wallrun", value] => next.wallrun = parse_bool("wallrun", value)?,
        ["doublejump" | "dj", value] => next.double_jump = parse_bool("doublejump", value)?,
        ["slide", value] => next.slide = parse_bool("slide", value)?,
        _ => return Err(format!("usage: {USAGE}")),
    }
    Ok(Some(next))
}

/// The `doublejump` shorthand: bare is a status query, `on`/`off` toggles it.
fn parse_double_jump(
    args: &[String],
    base: MovementTuning,
) -> Result<Option<MovementTuning>, String> {
    let mut next = base;
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => return Ok(None),
        [value] => next.double_jump = parse_bool("doublejump", value)?,
        _ => return Err("usage: doublejump [on|off]".into()),
    }
    Ok(Some(next))
}

/// The `slide` shorthand: bare is a status query, `on`/`off` toggles it.
fn parse_slide(args: &[String], base: MovementTuning) -> Result<Option<MovementTuning>, String> {
    let mut next = base;
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => return Ok(None),
        [value] => next.slide = parse_bool("slide", value)?,
        _ => return Err("usage: slide [on|off]".into()),
    }
    Ok(Some(next))
}

fn parse_bool(name: &str, raw: &str) -> Result<bool, String> {
    match raw {
        "on" | "1" | "true" => Ok(true),
        "off" | "0" | "false" => Ok(false),
        _ => Err(format!("{name}: expected on or off, got `{raw}`")),
    }
}

fn format_profile(prefix: &str, tuning: &MovementTuning) -> String {
    format!(
        "{prefix} surf={} wallrun={} doublejump={} slide={}",
        on_off(tuning.surf),
        on_off(tuning.wallrun),
        on_off(tuning.double_jump),
        on_off(tuning.slide),
    )
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn bare_is_status_query() {
        assert_eq!(parse_movement(&[], MovementTuning::default()), Ok(None));
    }

    #[test]
    fn fluid_turns_everything_on() {
        let next = parse_movement(&args(&["fluid"]), MovementTuning::default())
            .unwrap()
            .unwrap();
        assert!(next.surf && next.wallrun && next.double_jump && next.slide);
    }

    #[test]
    fn retail_turns_everything_off() {
        let next = parse_movement(&args(&["retail"]), MovementTuning::fluid())
            .unwrap()
            .unwrap();
        assert!(!next.surf && !next.wallrun && !next.double_jump && !next.slide);
    }

    #[test]
    fn doublejump_keeps_the_rest() {
        let next = parse_movement(&args(&["doublejump", "off"]), MovementTuning::fluid())
            .unwrap()
            .unwrap();
        assert!(next.surf && next.wallrun && !next.double_jump);
    }

    #[test]
    fn rejects_garbage() {
        let base = MovementTuning::default();
        assert!(parse_movement(&args(&["doublejump", "maybe"]), base).is_err());
        assert!(parse_movement(&args(&["nope"]), base).is_err());
    }

    #[test]
    fn doublejump_shorthand() {
        assert_eq!(parse_double_jump(&[], MovementTuning::default()), Ok(None));
        let on = parse_double_jump(&args(&["on"]), MovementTuning::default())
            .unwrap()
            .unwrap();
        assert!(on.double_jump);
        let off = parse_double_jump(&args(&["off"]), MovementTuning::fluid())
            .unwrap()
            .unwrap();
        assert!(!off.double_jump);
        assert!(parse_double_jump(&args(&["maybe"]), MovementTuning::default()).is_err());
        assert!(parse_double_jump(&args(&["on", "off"]), MovementTuning::default()).is_err());
    }

    #[test]
    fn slide_shorthand() {
        assert_eq!(parse_slide(&[], MovementTuning::default()), Ok(None));
        let on = parse_slide(&args(&["on"]), MovementTuning::default())
            .unwrap()
            .unwrap();
        assert!(on.slide);
        let off = parse_slide(&args(&["off"]), MovementTuning::fluid())
            .unwrap()
            .unwrap();
        assert!(!off.slide);
        assert!(parse_slide(&args(&["maybe"]), MovementTuning::default()).is_err());
        assert!(parse_slide(&args(&["on", "off"]), MovementTuning::default()).is_err());
    }
}
