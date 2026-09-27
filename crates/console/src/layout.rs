//! Mod: `layout` — pick a map layout and load its base map.

use bevy::prelude::*;

use crate::plugin::ConsoleCommandQueue;
use crate::{ConsoleCommand, ConsoleLine, ConsoleRegistry, ConsoleSettings, ConsoleState};

const USAGE: &str =
    "layout [<name>|off|reload] — load a map layout from layouts/ with its base map (mod)";

pub(crate) fn register_layout_commands(registry: &mut ConsoleRegistry) {
    if registry.resolve("clipprobe").is_none() {
        registry.register(
            crate::CommandSpec::new("clipprobe").usage(
                "clipprobe [dist] — trace the player box forward and name what it hits (mod)",
            ),
        );
    }
    if registry.resolve("layout").is_none() {
        let mut names = map_layout::list();
        names.extend(["off".to_owned(), "reload".to_owned()]);
        registry.register(
            crate::CommandSpec::new("layout")
                .usage(USAGE)
                .arg(crate::StaticCompleter::new(names)),
        );
    }
}

pub(crate) fn route_layout_commands(
    mut events: MessageReader<ConsoleCommand>,
    mut console: ResMut<ConsoleState>,
    settings: Res<ConsoleSettings>,
    mut line: ResMut<ConsoleLine>,
    mut queue: ResMut<ConsoleCommandQueue>,
    authority: Option<Res<net::AuthorityWorld>>,
    local: Res<net::LocalPresentClient>,
) {
    let capacity = settings.log_capacity;
    let mut echo = |msg: String| {
        diag::info!(Console, "{msg}");
        line.0 = msg.clone();
        console.echo(msg, capacity);
    };

    for cmd in events.read() {
        if cmd.name == "clipprobe" {
            let Some(authority) = authority.as_ref() else {
                echo("clipprobe: no authority world (not a listen host)".into());
                continue;
            };
            let Some(ps) = authority.0.player(local.0) else {
                echo("clipprobe: no local player".into());
                continue;
            };
            let dist = cmd
                .args
                .first()
                .and_then(|a| a.parse::<f32>().ok())
                .unwrap_or(64.0);
            let (sin, cos) = ps.viewangles[1].to_radians().sin_cos();
            let start = ps.origin;
            let end = [start[0] + cos * dist, start[1] + sin * dist, start[2]];
            let probe = authority.0.probe_player_clip(start, end);
            let describe = |t: &trace_iw4::Trace| {
                format!(
                    "frac={:.3} start/allsolid={}/{} contents={:#x} normal=({:.2} {:.2} {:.2})",
                    t.fraction,
                    t.startsolid,
                    t.allsolid,
                    t.contents,
                    t.normal[0],
                    t.normal[1],
                    t.normal[2]
                )
            };
            echo(format!(
                "clipprobe {dist:.0}u from ({:.0} {:.0} {:.0}) yaw {:.0} mask {:#x}: map {} | layout {}",
                start[0],
                start[1],
                start[2],
                ps.viewangles[1],
                probe.tracemask,
                describe(&probe.map),
                describe(&probe.layout)
            ));
            continue;
        }
        if cmd.name != "layout" {
            continue;
        }
        let name = match cmd.args.first().map(String::as_str) {
            None => {
                let active = map_layout::active().map_or_else(
                    || "none".to_owned(),
                    |l| format!("{} (base {})", l.name, l.base_map),
                );
                echo(format!(
                    "layout: active {active}; available: {}",
                    map_layout::list().join(", ")
                ));
                continue;
            }
            Some("off") => {
                map_layout::set_active(None);
                echo("layout: off (applies on the next map load)".into());
                continue;
            }
            Some("reload") => match map_layout::active() {
                Some(active) => active.name.clone(),
                None => {
                    echo("layout reload: no active layout".into());
                    continue;
                }
            },
            Some(name) => name.to_owned(),
        };
        let layout = match map_layout::load(&name) {
            Ok(layout) => layout,
            Err(error) => {
                echo(format!("layout: {error}"));
                continue;
            }
        };
        let base = layout.base_map.clone();
        echo(format!(
            "layout: {} — {} shapes on {base}; loading map",
            layout.name,
            layout.shapes.len()
        ));
        map_layout::set_active(Some(layout));
        if let Some(map) = ConsoleCommand::parse(&format!("map {base}")) {
            queue.0.push_front(map);
        }
    }
}
