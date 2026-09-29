//! On-screen music player: what is playing, a scrollable track list and the
//! controls.
//!
//! `F8` hides the panel, `TAB` enters the track list (the game stops reading
//! input while it is open), the arrow keys browse, `ENTER` plays the selected
//! track and `ESC` leaves the list. The bracket keys skip tracks, `\`
//! plays/pauses, `-`/`=` change the music volume and `;`/`'` the game sound.
//! The same panel toggles with `music ui on|off` from the console.

use bevy::{
    input::{ButtonState, keyboard::KeyCode, keyboard::KeyboardInput},
    prelude::*,
};
use ui::UiLayer;

use audio::{MusicPlayer, MusicRequest};

use crate::ConsoleState;

/// Rows of the list that are visible at once.
const ROWS: usize = 9;
const ROW_H: f32 = 26.0;
const PANEL_WIDTH: f32 = 430.0;
const VOLUME_STEP: f32 = 0.05;

const ACCENT: Color = Color::srgb(0.96, 0.62, 0.14);
const TEXT: Color = Color::srgb(0.91, 0.92, 0.94);
const MUTED: Color = Color::srgb(0.52, 0.55, 0.60);
const PANEL_BG: Color = Color::srgba(0.035, 0.045, 0.06, 0.86);
const ROW_SELECTED: Color = Color::srgba(0.96, 0.62, 0.14, 0.20);
const ROW_CURRENT: Color = Color::srgba(1.0, 1.0, 1.0, 0.06);
const TRACK_BG: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);

const HUD_FONT: &[u8] = include_bytes!("../assets/BebasNeue-Regular.ttf");

#[derive(Resource)]
pub(crate) struct MusicHud {
    pub visible: bool,
    browsing: bool,
    selected: usize,
    scroll: usize,
}

impl MusicHud {
    pub(crate) fn browsing(&self) -> bool {
        self.browsing
    }
}

impl Default for MusicHud {
    fn default() -> Self {
        Self {
            visible: true,
            browsing: false,
            selected: 0,
            scroll: 0,
        }
    }
}

#[derive(Component)]
pub(crate) struct MusicHudRoot;
#[derive(Component)]
pub(crate) struct MusicTitleText;
#[derive(Component)]
pub(crate) struct MusicCountText;
#[derive(Component)]
pub(crate) struct MusicNowText;
#[derive(Component)]
pub(crate) struct MusicRow(pub(crate) usize);
#[derive(Component)]
pub(crate) struct MusicRowText(pub(crate) usize);
#[derive(Component)]
pub(crate) struct MusicVolumeText;
#[derive(Component)]
pub(crate) struct MusicHintText;
#[derive(Component)]
pub(crate) struct MusicScrollThumb;

fn shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::splat(1.5),
        color: Color::linear_rgba(0., 0., 0., 0.8),
        ..default()
    }
}

fn hud_font(font: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: font.clone().into(),
        font_size: FontSize::Px(size),
        ..default()
    }
}

pub(crate) fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let font = fonts.add(Font::from_bytes(HUD_FONT.to_vec()));
    commands
        .spawn((
            MusicHudRoot,
            UiLayer::Overlay,
            Node {
                position_type: PositionType::Absolute,
                right: px(14),
                top: px(14),
                width: px(PANEL_WIDTH),
                padding: UiRect::all(px(10)),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(Color::srgba(0.96, 0.62, 0.14, 0.55)),
            GlobalZIndex(19_000),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Baseline,
                    ..default()
                })
                .with_children(|header| {
                    header.spawn((
                        MusicTitleText,
                        Text::new("MUSIC"),
                        hud_font(&font, 22.0),
                        TextColor(ACCENT),
                        shadow(),
                    ));
                    header.spawn((
                        MusicCountText,
                        Text::new("0 / 0"),
                        hud_font(&font, 16.0),
                        TextColor(MUTED),
                    ));
                });

            panel.spawn((
                MusicNowText,
                Text::new(""),
                hud_font(&font, 19.0),
                TextColor(TEXT),
                shadow(),
            ));

            panel.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: px(1),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.96, 0.62, 0.14, 0.35)),
            ));

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    height: px(ROW_H * ROWS as f32),
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                    ..default()
                })
                .with_children(|list| {
                    list.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        ..default()
                    })
                    .with_children(|column| {
                        for i in 0..ROWS {
                            column
                                .spawn((
                                    MusicRow(i),
                                    Node {
                                        width: Val::Percent(100.0),
                                        height: px(ROW_H),
                                        padding: UiRect::horizontal(px(6)),
                                        align_items: AlignItems::Center,
                                        border: UiRect::left(px(3)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::NONE),
                                    BorderColor {
                                        left: Color::NONE,
                                        ..default()
                                    },
                                ))
                                .with_children(|row| {
                                    row.spawn((
                                        MusicRowText(i),
                                        Text::new(""),
                                        hud_font(&font, 17.0),
                                        TextColor(MUTED),
                                    ));
                                });
                        }
                    });
                    list.spawn((
                        Node {
                            width: px(4),
                            height: Val::Percent(100.0),
                            border_radius: BorderRadius::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(TRACK_BG),
                    ))
                    .with_children(|track| {
                        track.spawn((
                            MusicScrollThumb,
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Percent(100.0),
                                top: px(0),
                                height: px(20),
                                border_radius: BorderRadius::all(px(2)),
                                ..default()
                            },
                            BackgroundColor(ACCENT),
                        ));
                    });
                });

            panel.spawn((
                MusicVolumeText,
                Text::new(""),
                hud_font(&font, 16.0),
                TextColor(MUTED),
            ));
            panel.spawn((
                MusicHintText,
                Text::new("TAB LIST   [ ] SKIP   \\ PAUSE   - = VOL   ; ' GAME   F8 HIDE"),
                hud_font(&font, 14.0),
                TextColor(MUTED),
            ));
        });
}

fn clamp_scroll(hud: &mut MusicHud, count: usize) {
    let max_scroll = count.saturating_sub(ROWS);
    hud.scroll = hud.scroll.min(max_scroll);
    if hud.selected < hud.scroll {
        hud.scroll = hud.selected;
    }
    if hud.selected >= hud.scroll + ROWS {
        hud.scroll = hud.selected + 1 - ROWS;
    }
    hud.scroll = hud.scroll.min(max_scroll);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_music_hud(
    mut events: MessageReader<KeyboardInput>,
    console: Res<ConsoleState>,
    mut hud: ResMut<MusicHud>,
    mut player: ResMut<MusicPlayer>,
    mut game: ResMut<frame::GameSettings>,
    mut roots: Query<&mut Visibility, With<MusicHudRoot>>,
    mut title: Query<(&mut Text, &mut TextColor), (With<MusicTitleText>, Without<MusicRowText>)>,
    mut count: Query<
        &mut Text,
        (
            With<MusicCountText>,
            Without<MusicTitleText>,
            Without<MusicRowText>,
        ),
    >,
    mut now: Query<
        &mut Text,
        (
            With<MusicNowText>,
            Without<MusicTitleText>,
            Without<MusicCountText>,
            Without<MusicRowText>,
        ),
    >,
    mut rows: Query<(&MusicRow, &mut BackgroundColor, &mut BorderColor)>,
    mut row_texts: Query<
        (&MusicRowText, &mut Text, &mut TextColor),
        (With<MusicRowText>, Without<MusicTitleText>),
    >,
    mut volume: Query<
        &mut Text,
        (
            With<MusicVolumeText>,
            Without<MusicTitleText>,
            Without<MusicCountText>,
            Without<MusicNowText>,
            Without<MusicRowText>,
        ),
    >,
    mut thumbs: Query<&mut Node, With<MusicScrollThumb>>,
) {
    let count_total = player.track_count();
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
                    clamp_scroll(&mut hud, count_total);
                }
            }
            KeyCode::ArrowUp if hud.browsing => {
                hud.selected = hud.selected.saturating_sub(1);
                clamp_scroll(&mut hud, count_total);
            }
            KeyCode::ArrowDown if hud.browsing => {
                if hud.selected + 1 < count_total {
                    hud.selected += 1;
                }
                clamp_scroll(&mut hud, count_total);
            }
            KeyCode::PageUp if hud.browsing => {
                hud.selected = hud.selected.saturating_sub(ROWS);
                clamp_scroll(&mut hud, count_total);
            }
            KeyCode::PageDown if hud.browsing => {
                hud.selected = (hud.selected + ROWS).min(count_total.saturating_sub(1));
                clamp_scroll(&mut hud, count_total);
            }
            KeyCode::Home if hud.browsing => {
                hud.selected = 0;
                clamp_scroll(&mut hud, count_total);
            }
            KeyCode::End if hud.browsing => {
                hud.selected = count_total.saturating_sub(1);
                clamp_scroll(&mut hud, count_total);
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
    if !hud.visible {
        return;
    }

    let title_color = if hud.browsing { ACCENT } else { ACCENT };
    for (mut text, mut color) in &mut title {
        if text.as_str() != "MUSIC" {
            text.clear();
            text.push_str("MUSIC");
        }
        if color.0 != title_color {
            *color = TextColor(title_color);
        }
    }
    for mut text in &mut count {
        let body = format!("{} / {}", current + 1, count_total);
        text.clear();
        text.push_str(&body);
    }
    let now_name = player
        .now_playing()
        .map(str::to_uppercase)
        .unwrap_or_else(|| "NOTHING PLAYING".to_owned());
    for mut text in &mut now {
        if text.as_str() != now_name {
            text.clear();
            text.push_str(&now_name);
        }
    }

    for (row, mut bg, mut border) in &mut rows {
        let index = hud.scroll + row.0;
        let selected = hud.browsing && index == hud.selected;
        let want_bg = if selected {
            ROW_SELECTED
        } else if index == current && index < count_total {
            ROW_CURRENT
        } else {
            Color::NONE
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want_border = if selected { ACCENT } else { Color::NONE };
        if border.left != want_border {
            border.left = want_border;
        }
    }
    for (row, mut text, mut color) in &mut row_texts {
        let index = hud.scroll + row.0;
        let (body, want_color) = if index < count_total {
            let name = player.track_name(index).unwrap_or("").to_uppercase();
            let mark = if index == current { ">" } else { " " };
            (
                format!("{mark} {:>3}  {name}", index + 1),
                if index == current {
                    ACCENT
                } else if hud.browsing && index == hud.selected {
                    TEXT
                } else {
                    MUTED
                },
            )
        } else {
            (String::new(), MUTED)
        };
        if text.as_str() != body {
            text.clear();
            text.push_str(&body);
        }
        if color.0 != want_color {
            *color = TextColor(want_color);
        }
    }
    for mut text in &mut volume {
        let body = format!(
            "MUSIC {:.2}    SOUND {:.2}    {}",
            player.volume,
            game.master_volume,
            if player.enabled { "PLAYING" } else { "PAUSED" }
        );
        text.clear();
        text.push_str(&body);
    }

    let max_scroll = count_total.saturating_sub(ROWS);
    let list_h = ROW_H * ROWS as f32;
    let ratio = if count_total == 0 {
        1.0
    } else {
        (ROWS as f32 / count_total as f32).min(1.0)
    };
    let thumb_h = (list_h * ratio).max(ROW_H).min(list_h);
    let t = if max_scroll == 0 {
        0.0
    } else {
        hud.scroll as f32 / max_scroll as f32
    };
    for mut node in &mut thumbs {
        node.height = px(thumb_h);
        node.top = px((list_h - thumb_h) * t);
    }
}
