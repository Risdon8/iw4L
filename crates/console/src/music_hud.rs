//! Music panel input. The panel itself is drawn by the HUD on the game's font
//! pipeline (`hud::music_panel`); this only reads keys and maintains the shared
//! [`MusicHudState`].
//!
//! `F8` hides the panel, `TAB` opens the track list (the game stops reading
//! input while it is open), the arrow keys browse, `ENTER` plays the selection
//! and `ESC` leaves the list. The bracket keys skip tracks, `\` plays/pauses,
//! `-`/`=` change the music volume and `;`/`'` the game sound.

use bevy::{
    input::{ButtonState, keyboard::KeyCode, keyboard::KeyboardInput},
    prelude::*,
};

use audio::{MUSIC_HUD_ROWS, MusicHudState, MusicPlayer, MusicRequest};

use crate::ConsoleState;

const VOLUME_STEP: f32 = 0.05;

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_music_hud(
    mut events: MessageReader<KeyboardInput>,
    console: Res<ConsoleState>,
    mut hud: ResMut<MusicHudState>,
    mut player: ResMut<MusicPlayer>,
    mut game: ResMut<frame::GameSettings>,
) {
    let count = player.track_count();
    let current = player.current_index();

    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        match event.key_code {
            KeyCode::F8 => {
                hud.visible = !hud.visible;
                if !hud.visible {
                    hud.browsing = false;
                }
                diag::info!(
                    Console,
                    "music: HUD {}",
                    if hud.visible { "shown" } else { "hidden" }
                );
            }
            KeyCode::Tab if hud.visible => {
                hud.browsing = !hud.browsing;
                if hud.browsing {
                    hud.selected = current;
                    hud.clamp_scroll(count);
                }
            }
            KeyCode::ArrowUp if hud.browsing => {
                hud.selected = hud.selected.saturating_sub(1);
                hud.clamp_scroll(count);
            }
            KeyCode::ArrowDown if hud.browsing => {
                if hud.selected + 1 < count {
                    hud.selected += 1;
                }
                hud.clamp_scroll(count);
            }
            KeyCode::PageUp if hud.browsing => {
                hud.selected = hud.selected.saturating_sub(MUSIC_HUD_ROWS);
                hud.clamp_scroll(count);
            }
            KeyCode::PageDown if hud.browsing => {
                hud.selected = (hud.selected + MUSIC_HUD_ROWS).min(count.saturating_sub(1));
                hud.clamp_scroll(count);
            }
            KeyCode::Home if hud.browsing => {
                hud.selected = 0;
                hud.clamp_scroll(count);
            }
            KeyCode::End if hud.browsing => {
                hud.selected = count.saturating_sub(1);
                hud.clamp_scroll(count);
            }
            KeyCode::Enter if hud.browsing => {
                player.request(MusicRequest::Track(hud.selected));
            }
            KeyCode::Escape if hud.browsing => {
                hud.browsing = false;
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
