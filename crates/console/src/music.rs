//! Mod: `music` — the local folder music player.
//!
//! Plays the tracks the user drops into `music/` beside the executable. This
//! command only sets state or queues an action; [`audio`] owns playback.

use bevy::prelude::*;
use audio::{MusicPlayer, MusicRequest, Repeat};

use crate::{ConsoleCommand, ConsoleLine, ConsoleRegistry, ConsoleSettings, ConsoleState};

const USAGE: &str = "music | music on|off|toggle|next|prev|list | music volume <0-1> | music shuffle [on|off] | music repeat <off|all|one> | music track <n> | music reload — local music/ folder (mod)";

pub(crate) fn register_music_commands(registry: &mut ConsoleRegistry) {
    if registry.resolve("music").is_none() {
        registry.register(crate::CommandSpec::new("music").usage(USAGE).arg(
            crate::StaticCompleter::new([
                "on", "off", "toggle", "next", "prev", "list", "volume", "shuffle", "repeat",
                "track", "reload",
            ]),
        ));
    }
}

pub(crate) fn route_music_commands(
    mut events: MessageReader<ConsoleCommand>,
    mut console: ResMut<ConsoleState>,
    settings: Res<ConsoleSettings>,
    mut line: ResMut<ConsoleLine>,
    mut player: ResMut<MusicPlayer>,
) {
    let capacity = settings.log_capacity;
    let echo = |msg: String, console: &mut ConsoleState, line: &mut ConsoleLine| {
        diag::info!(Console, "{msg}");
        line.0 = msg.clone();
        console.echo(msg, capacity);
    };

    for cmd in events.read() {
        if cmd.name != "music" {
            continue;
        }
        let args: Vec<String> = cmd
            .args
            .iter()
            .map(|arg| arg.to_ascii_lowercase())
            .collect();
        let Some(sub) = args.first().map(String::as_str) else {
            echo(player.summary(), &mut console, &mut line);
            continue;
        };
        match sub {
            "on" => {
                player.request(MusicRequest::On);
                echo(player.summary(), &mut console, &mut line);
            }
            "off" => {
                player.request(MusicRequest::Off);
                echo("music: stopping".to_owned(), &mut console, &mut line);
            }
            "toggle" => {
                player.request(MusicRequest::Toggle);
                echo(
                    format!("music: {}", if player.enabled { "stopping" } else { "starting" }),
                    &mut console,
                    &mut line,
                );
            }
            "next" => {
                player.request(MusicRequest::Next);
                echo("music: next".to_owned(), &mut console, &mut line);
            }
            "prev" => {
                player.request(MusicRequest::Prev);
                echo("music: previous".to_owned(), &mut console, &mut line);
            }
            "reload" => {
                player.request(MusicRequest::Reload);
                echo("music: rescanning folder".to_owned(), &mut console, &mut line);
            }
            "track" => match args.get(1).and_then(|n| n.parse::<usize>().ok()) {
                Some(n) if n >= 1 && n <= player.track_count() => {
                    player.request(MusicRequest::Track(n - 1));
                    echo(format!("music: track {n}"), &mut console, &mut line);
                }
                _ => echo(
                    format!("music: track <1-{}>", player.track_count()),
                    &mut console,
                    &mut line,
                ),
            },
            "list" => {
                if player.track_count() == 0 {
                    echo(
                        format!("music: no tracks in {}", player.directory().display()),
                        &mut console,
                        &mut line,
                    );
                    continue;
                }
                let current = player.current_index();
                let lines: Vec<String> = player
                    .track_names()
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        format!("{:>3}. {}{}", index + 1, name, if index == current { "  <==" } else { "" })
                    })
                    .collect();
                for entry in lines {
                    echo(entry, &mut console, &mut line);
                }
            }
            "volume" => match args.get(1) {
                Some(value) => match value.parse::<f32>() {
                    Ok(value) if (0.0..=1.0).contains(&value) => {
                        player.volume = value;
                        echo(format!("music: volume {value:.2}"), &mut console, &mut line);
                    }
                    _ => echo("music: volume <0-1>".to_owned(), &mut console, &mut line),
                },
                None => echo(
                    format!("music: volume {:.2}", player.volume),
                    &mut console,
                    &mut line,
                ),
            },
            "shuffle" => match args.get(1).map(String::as_str) {
                Some("on") => {
                    player.shuffle = true;
                    echo("music: shuffle on".to_owned(), &mut console, &mut line);
                }
                Some("off") => {
                    player.shuffle = false;
                    echo("music: shuffle off".to_owned(), &mut console, &mut line);
                }
                None => echo(
                    format!("music: shuffle {}", if player.shuffle { "on" } else { "off" }),
                    &mut console,
                    &mut line,
                ),
                Some(_) => echo("music: shuffle [on|off]".to_owned(), &mut console, &mut line),
            },
            "repeat" => match args.get(1).map(String::as_str) {
                Some("off") => {
                    player.repeat = Repeat::Off;
                    echo("music: repeat off".to_owned(), &mut console, &mut line);
                }
                Some("all") => {
                    player.repeat = Repeat::All;
                    echo("music: repeat all".to_owned(), &mut console, &mut line);
                }
                Some("one") => {
                    player.repeat = Repeat::One;
                    echo("music: repeat one".to_owned(), &mut console, &mut line);
                }
                _ => echo("music: repeat <off|all|one>".to_owned(), &mut console, &mut line),
            },
            other => echo(
                format!("music: unknown option `{other}`\n{USAGE}"),
                &mut console,
                &mut line,
            ),
        }
    }
}
