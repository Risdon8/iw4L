use input_iw4::{ClientInput, command_id_lookup, command_name, key_event};

use std::collections::HashMap;

use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::{MouseButton, MouseScrollUnit};
use bevy::prelude::Resource;

pub const DEFAULT_CONTROLS: &str = include_str!("../assets/default_controls.cfg");

mod key_names;

pub use key_names::{
    BINDABLE_KEYS, display_button, host_keynum, parse_button_name, parse_key_name,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindButton {
    Key(KeyCode),
    Mouse(MouseButton),
    WheelUp,
    WheelDown,
}

pub(crate) fn wheel_button(y: f32) -> Option<BindButton> {
    if y > 0.0 {
        Some(BindButton::WheelUp)
    } else if y < 0.0 {
        Some(BindButton::WheelDown)
    } else {
        None
    }
}

pub(crate) fn wheel_detents(unit: MouseScrollUnit, y: f32, carry: &mut f32) -> i32 {
    if !y.is_finite() {
        return 0;
    }
    let delta = match unit {
        MouseScrollUnit::Line => y,
        MouseScrollUnit::Pixel => y / 100.0,
    };
    *carry = (*carry + delta).clamp(-32.0, 32.0);
    let detents = carry.trunc() as i32;
    *carry -= detents as f32;
    detents
}

pub struct BindInputs<'a> {
    pub keys: &'a ButtonInput<KeyCode>,
    pub mouse: &'a ButtonInput<MouseButton>,
}

impl<'a> BindInputs<'a> {
    pub fn new(keys: &'a ButtonInput<KeyCode>, mouse: &'a ButtonInput<MouseButton>) -> Self {
        Self { keys, mouse }
    }

    pub fn pressed(&self, button: BindButton) -> bool {
        match button {
            BindButton::Key(key) => self.keys.pressed(key),
            BindButton::Mouse(btn) => self.mouse.pressed(btn),
            BindButton::WheelUp | BindButton::WheelDown => false,
        }
    }

    pub fn just_pressed(&self, button: BindButton) -> bool {
        match button {
            BindButton::Key(key) => self.keys.just_pressed(key),
            BindButton::Mouse(btn) => self.mouse.just_pressed(btn),
            BindButton::WheelUp | BindButton::WheelDown => false,
        }
    }

    pub fn just_released(&self, button: BindButton) -> bool {
        match button {
            BindButton::Key(key) => self.keys.just_released(key),
            BindButton::Mouse(btn) => self.mouse.just_released(btn),
            BindButton::WheelUp | BindButton::WheelDown => false,
        }
    }
}

#[derive(Resource, Debug, Clone, Default)]
pub struct KeyBinds {
    map: HashMap<BindButton, u32>,
}

impl KeyBinds {
    pub fn apply_defaults(&mut self) {
        self.map.clear();
        let _ = self.apply_script(DEFAULT_CONTROLS);
    }

    pub fn apply_script(&mut self, script: &str) -> Vec<String> {
        self.apply_script_inner(script, true)
    }

    pub(crate) fn apply_config_script(&mut self, script: &str) -> Vec<String> {
        self.apply_script_inner(script, false)
    }

    fn apply_script_inner(&mut self, script: &str, echo_success: bool) -> Vec<String> {
        let mut output = Vec::new();
        for raw in script.split([';', '\n']) {
            let line = raw.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            let Some(command) = crate::ConsoleCommand::parse(line) else {
                continue;
            };
            match command.name.as_str() {
                "bind" => match self.cmd_bind(&command.args) {
                    Ok(Some(msg)) if echo_success => output.push(msg),
                    Ok(Some(_)) => {}
                    Ok(None) => {}
                    Err(msg) => output.push(msg),
                },
                "unbind" => match self.cmd_unbind(&command.args) {
                    Ok(Some(msg)) if echo_success => output.push(msg),
                    Ok(Some(_)) => {}
                    Ok(None) => {}
                    Err(msg) => output.push(msg),
                },
                "unbindall" => {
                    self.map.clear();
                    if echo_success {
                        output.push("unbindall".into());
                    }
                }
                other => output.push(format!("unknown bind-script command `{other}`")),
            }
        }
        output
    }

    pub fn set(&mut self, button: BindButton, id: u32) {
        self.map.insert(button, id);
    }

    pub fn clear_button(&mut self, button: BindButton) -> bool {
        self.map.remove(&button).is_some()
    }

    pub fn clear_command(&mut self, id: u32) -> bool {
        let before = self.map.len();
        self.map.retain(|_, bound| *bound != id);
        self.map.len() != before
    }

    pub fn clear_all(&mut self) {
        self.map.clear();
    }

    pub fn get(&self, button: BindButton) -> Option<u32> {
        self.map.get(&button).copied()
    }

    pub fn binding_name(&self, button: BindButton) -> Option<&'static str> {
        self.get(button).and_then(command_name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (BindButton, u32)> + '_ {
        self.map.iter().map(|(b, id)| (*b, *id))
    }

    pub fn list_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .map
            .iter()
            .filter_map(|(button, id)| {
                command_name(*id).map(|name| format!("bind {} {name}", display_button(*button)))
            })
            .collect();
        lines.sort();
        lines.dedup();
        lines
    }

    fn cmd_bind(&mut self, args: &[String]) -> Result<Option<String>, String> {
        match args {
            [] => Ok(None),
            [key] => {
                let buttons =
                    parse_button_name(key).ok_or_else(|| format!("unknown key `{key}`"))?;
                let names: Vec<&str> = buttons
                    .iter()
                    .filter_map(|button| self.binding_name(*button))
                    .collect();
                if names.is_empty() {
                    Ok(Some(format!("`{key}` is unbound")))
                } else {
                    Ok(Some(format!("bind {key} {}", names[0])))
                }
            }
            [key, action @ ..] => {
                let action = action.join(" ");
                let id = command_id_lookup(&action)
                    .ok_or_else(|| format!("unknown command `{action}`"))?;
                let buttons =
                    parse_button_name(key).ok_or_else(|| format!("unknown key `{key}`"))?;
                for button in buttons {
                    self.set(button, id);
                }
                let name = command_name(id).unwrap_or(action.as_str());
                Ok(Some(format!("bind {key} {name}")))
            }
        }
    }

    fn cmd_unbind(&mut self, args: &[String]) -> Result<Option<String>, String> {
        match args {
            [key] => {
                let buttons =
                    parse_button_name(key).ok_or_else(|| format!("unknown key `{key}`"))?;
                let mut any = false;
                for button in buttons {
                    any |= self.clear_button(button);
                }
                if any {
                    Ok(Some(format!("unbind {key}")))
                } else {
                    Ok(Some(format!("`{key}` is unbound")))
                }
            }
            _ => Err("usage: unbind <key>".into()),
        }
    }
}

pub(crate) fn pulse_wheel_binding(
    binds: &KeyBinds,
    client: &mut ClientInput,
    button: BindButton,
    now_msec: i32,
    frame_msec: u32,
) {
    let Some(id) = binds.get(button) else { return };
    let key_num = host_keynum(button);
    client.keys[key_num].binding = id;
    key_event(client, key_num, true, now_msec, frame_msec);
    key_event(client, key_num, false, now_msec, frame_msec);
}
