use bevy::prelude::*;

#[derive(Resource, Default, Debug)]
pub struct UiPartyState {
    pub active: bool,
    pub in_lobby: bool,
    pub is_host: bool,
}

#[derive(Resource, Default, Debug)]
pub struct UiMenuDvars {
    values: std::collections::HashMap<String, String>,
}

impl UiMenuDvars {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        self.values.insert(name.to_ascii_lowercase(), value.into());
    }
}

#[derive(Resource, Default)]
pub struct UiBindingCapture {
    pub command: Option<String>,
    pub consumed_input: bool,
}

#[derive(Message, Clone, Debug)]
pub struct UiBindRequest {
    pub command: String,
}

#[derive(Message, Clone, Debug)]
pub struct UiPlaySound {
    pub alias: String,
}

#[derive(Message, Clone, Debug)]
pub struct UiPlayMusic {
    pub alias: String,
}

#[derive(Message, Clone, Debug, Default)]
pub struct UiStopMusic;

#[derive(Message, Clone, Debug)]
pub struct UiExecCommand {
    pub text: String,
}

#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub enum UiMenuRequest {
    Toggle,
    Open(String),
    Close(String),
    Key(UiMenuKey),
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiMenuKey {
    Escape,
    Enter,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Backspace,
    Delete,
}

pub fn register_ui_sound(app: &mut App) {
    app.init_resource::<UiMenuDvars>()
        .init_resource::<UiBindingCapture>()
        .init_resource::<UiPartyState>()
        .add_message::<UiBindRequest>()
        .add_message::<UiPlaySound>()
        .add_message::<UiPlayMusic>()
        .add_message::<UiStopMusic>()
        .add_message::<UiExecCommand>()
        .add_message::<UiMenuRequest>();
}
