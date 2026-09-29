//! Music panel input. The panel itself is drawn by the HUD on the game's font
//! pipeline (`hud::music_panel`); this only reads keys and keeps the shared
//! [`MusicHudState`] and [`MusicPlayer`] in sync.
//!
//! `F8` hides the panel, the bracket keys skip tracks, `\` plays/pauses, `-`/`=`
//! change the music volume and `;`/`'` the game sound. Track selection is
//! `music track <n>` (or `music list`) from the console.

use bevy::{
    input::{ButtonState, keyboard::KeyCode, keyboard::KeyboardInput},
    prelude::*,
};

use audio::{MusicHudState, MusicPlayer, MusicRequest};

use crate::ConsoleState;

const VOLUME_STEP: f32 = 0.05;

pub(crate) fn update_music_hud(
    mut events: MessageReader<KeyboardInput>,
    console: Res<ConsoleState>,
    mut hud: ResMut<MusicHudState>,
    mut player: ResMut<MusicPlayer>,
    mut game: ResMut<frame::GameSettings>,
) {
    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        match event.key_code {
            KeyCode::F8 => {
                hud.visible = !hud.visible;
                diag::info!(
                    Console,
                    "music: HUD {}",
                    if hud.visible { "shown" } else { "hidden" }
                );
            }
            _ if console.open => {}
            KeyCode::BracketLeft => player.request(MusicRequest::Prev),
            KeyCode::BracketRight => player.request(MusicRequest::Next),
            KeyCode::Backslash => player.request(MusicRequest::Toggle),
            KeyCode::Minus => player.volume = (player.volume - VOLUME_STEP).max(0.0),
            KeyCode::Equal => player.volume = (player.volume + VOLUME_STEP).min(1.0),
            KeyCode::Semicolon => {
                game.master_volume = (game.master_volume - VOLUME_STEP).max(0.0);
            }
            KeyCode::Quote => {
                game.master_volume = (game.master_volume + VOLUME_STEP).min(1.0);
            }
            _ => {}
        }
    }
}
