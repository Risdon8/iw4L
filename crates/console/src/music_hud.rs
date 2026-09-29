//! On-screen music player: what is playing and its controls.
//!
//! A small panel in the top-right corner of the HUD. `F8` hides it; while it
//! is visible the bracket keys skip tracks, `\` plays/pauses, `-`/`=` change
//! the music volume and `;`/`'` change the game sound volume. The same panel
//! can be opened with `music ui on|off` from the console.

use bevy::prelude::*;
use ui::UiLayer;

use audio::{MusicPlayer, MusicRequest};

use crate::{ConsoleState, plugin::EMBEDDED_FONT};

const FONT_SIZE: f32 = 14.0;
const VOLUME_STEP: f32 = 0.05;

#[derive(Resource)]
pub(crate) struct MusicHud {
    pub visible: bool,
}

impl Default for MusicHud {
    fn default() -> Self {
        Self { visible: true }
    }
}

#[derive(Component)]
pub(crate) struct MusicHudRoot;

#[derive(Component)]
pub(crate) struct MusicHudText;

pub(crate) fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = fonts.add(Font::from_bytes(EMBEDDED_FONT.to_vec()));
    commands
        .spawn((
            MusicHudRoot,
            UiLayer::Overlay,
            Node {
                position_type: PositionType::Absolute,
                right: px(12),
                top: px(12),
                padding: UiRect::all(px(8)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.04, 0.72)),
            GlobalZIndex(19_000),
        ))
        .with_children(|panel| {
            panel.spawn((
                MusicHudText,
                Text::new(""),
                TextFont {
                    font: font.into(),
                    font_size: FontSize::Px(FONT_SIZE),
                    ..default()
                },
                TextColor(Color::srgb(0.82, 0.92, 0.82)),
            ));
        });
}

fn body(player: &MusicPlayer, sound: f32) -> String {
    let now = player.now_playing().unwrap_or("nothing playing");
    let count = player.track_count();
    let index = if count == 0 {
        "0/0".to_owned()
    } else {
        format!("{}/{}", player.current_index() + 1, count)
    };
    let state = if player.enabled { "" } else { " (paused)" };
    format!(
        "MUSIC  {now}{state}  [{index}]\n\
         music {:.2}  sound {:.2}\n\
         [ prev   ] next   \\ play/pause   - = music   ; ' sound   F8 hide",
        player.volume, sound
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_music_hud(
    keys: Res<ButtonInput<KeyCode>>,
    console: Res<ConsoleState>,
    mut hud: ResMut<MusicHud>,
    mut player: ResMut<MusicPlayer>,
    mut game: ResMut<frame::GameSettings>,
    mut roots: Query<&mut Visibility, With<MusicHudRoot>>,
    mut texts: Query<&mut Text, With<MusicHudText>>,
) {
    if keys.just_pressed(KeyCode::F8) {
        hud.visible = !hud.visible;
        diag::info!(
            Console,
            "music: HUD {}",
            if hud.visible { "shown" } else { "hidden" }
        );
    }
    if !console.open {
        if keys.just_pressed(KeyCode::BracketLeft) {
            player.request(MusicRequest::Prev);
        }
        if keys.just_pressed(KeyCode::BracketRight) {
            player.request(MusicRequest::Next);
        }
        if keys.just_pressed(KeyCode::Backslash) {
            player.request(MusicRequest::Toggle);
        }
        if keys.just_pressed(KeyCode::Minus) {
            player.volume = (player.volume - VOLUME_STEP).max(0.0);
        }
        if keys.just_pressed(KeyCode::Equal) {
            player.volume = (player.volume + VOLUME_STEP).min(1.0);
        }
        if keys.just_pressed(KeyCode::Semicolon) {
            game.master_volume = (game.master_volume - VOLUME_STEP).max(0.0);
        }
        if keys.just_pressed(KeyCode::Quote) {
            game.master_volume = (game.master_volume + VOLUME_STEP).min(1.0);
        }
    }

    let want = if hud.visible {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut roots {
        if *visibility != want {
            *visibility = want;
        }
    }
    if hud.visible {
        let body = body(&player, game.master_volume);
        for mut text in &mut texts {
            text.clear();
            text.push_str(&body);
        }
    }
}
