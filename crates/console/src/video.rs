//! Mod: `fullscreen` and `resolution` — the window mode and size.
//!
//! These just write [`frame::GameSettings`]; the UI's `apply_window_settings`
//! picks the change up on the next frame, and the change is saved to
//! `settings.cfg` like any other setting.

use bevy::prelude::*;

use crate::{ConsoleCommand, ConsoleLine, ConsoleRegistry, ConsoleSettings, ConsoleState};

const USAGE_FULLSCREEN: &str = "fullscreen [on|off] — borderless fullscreen or windowed (mod)";
const USAGE_RESOLUTION: &str = "resolution [<w>x<h>] — window size, e.g. 2560x1440 (mod)";

pub(crate) fn register_video_commands(registry: &mut ConsoleRegistry) {
    if registry.resolve("fullscreen").is_none() {
        registry.register(
            crate::CommandSpec::new("fullscreen")
                .usage(USAGE_FULLSCREEN)
                .arg(crate::StaticCompleter::new(["on", "off"])),
        );
    }
    if registry.resolve("resolution").is_none() {
        registry.register(
            crate::CommandSpec::new("resolution")
                .usage(USAGE_RESOLUTION)
                .arg(crate::StaticCompleter::new([
                    "1280x720",
                    "1600x900",
                    "1920x1080",
                    "2560x1440",
                    "3840x2160",
                ])),
        );
    }
}

pub(crate) fn route_video_commands(
    mut events: MessageReader<ConsoleCommand>,
    mut console: ResMut<ConsoleState>,
    settings: Res<ConsoleSettings>,
    mut line: ResMut<ConsoleLine>,
    mut game: ResMut<frame::GameSettings>,
) {
    let capacity = settings.log_capacity;
    let echo = |msg: String, console: &mut ConsoleState, line: &mut ConsoleLine| {
        diag::info!(Console, "{msg}");
        line.0 = msg.clone();
        console.echo(msg, capacity);
    };

    for cmd in events.read() {
        match cmd.name.as_str() {
            "fullscreen" => match cmd.args.first() {
                Some(value) => {
                    let value = value.to_ascii_lowercase();
                    let on = matches!(value.as_str(), "on" | "1" | "true" | "yes");
                    let off = matches!(value.as_str(), "off" | "0" | "false" | "no");
                    if !on && !off {
                        echo(USAGE_FULLSCREEN.to_owned(), &mut console, &mut line);
                        continue;
                    }
                    game.fullscreen = on;
                    echo(
                        format!(
                            "fullscreen {}",
                            if on { "on (borderless)" } else { "off (windowed)" }
                        ),
                        &mut console,
                        &mut line,
                    );
                }
                None => echo(
                    format!("fullscreen {}", if game.fullscreen { "on" } else { "off" }),
                    &mut console,
                    &mut line,
                ),
            },
            "resolution" => match cmd.args.first() {
                Some(value) => {
                    let parsed = value.split_once(['x', 'X']).and_then(|(w, h)| {
                        Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
                    });
                    let Some((width, height)) = parsed else {
                        echo(USAGE_RESOLUTION.to_owned(), &mut console, &mut line);
                        continue;
                    };
                    game.resolution = frame::DisplayResolution::new(width, height);
                    game.sanitize();
                    echo(
                        format!(
                            "resolution {}x{}",
                            game.resolution.width, game.resolution.height
                        ),
                        &mut console,
                        &mut line,
                    );
                }
                None => echo(
                    format!("resolution {}", game.resolution),
                    &mut console,
                    &mut line,
                ),
            },
            _ => {}
        }
    }
}
