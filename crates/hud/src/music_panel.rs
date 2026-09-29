//! The on-screen music panel, drawn on the HUD's own font pipeline so it uses
//! the game's `fonts/hudsmallfont` glyphs and the `white` chrome image for its
//! boxes (the same path `use_hint` and the compass use).
//!
//! Input lives in the console; this only reads [`audio::MusicHudState`] and
//! [`audio::MusicPlayer`].

use std::collections::HashMap;

use asset_game::MenuCatalog;
use bevy::prelude::*;

use crate::draw2d::{Draw2dCmd, Draw2dList, Draw2dOp, Draw2dProvenance, tessellate_fonts};
use crate::gaps::{GapCause, HudPresentationGaps};
use crate::gpu_list::{HudTessPass, TessJob};

#[derive(Component)]
pub(crate) struct MusicPanelRaster;

const FONT: &str = "fonts/hudsmallfont";
const ALIGN_RIGHT: i32 = 3; // hud_iw4::ALIGN_VIEWABLE_MAX
const ALIGN_TOP: i32 = 1; // hud_iw4::ALIGN_VIEWABLE

// Virtual (640x480) geometry.
const PANEL_W: f32 = 214.0;
const PANEL_MARGIN: f32 = 6.0;
const PAD: f32 = 6.0;
const HEADER_H: f32 = 15.0;
const ROW_H: f32 = 13.0;
const FOOTER_H: f32 = 13.0;
const NAME_CHARS: usize = 30;

const BG: [f32; 4] = [0.025, 0.035, 0.05, 0.74];
const BORDER: [f32; 4] = [0.96, 0.62, 0.14, 0.85];
const ACCENT: [f32; 4] = [0.96, 0.62, 0.14, 1.0];
const TEXT: [f32; 4] = [0.92, 0.93, 0.95, 1.0];
const MUTED: [f32; 4] = [0.55, 0.58, 0.62, 1.0];
const ROW_SELECTED: [f32; 4] = [0.96, 0.62, 0.14, 0.24];
const ROW_CURRENT: [f32; 4] = [1.0, 1.0, 1.0, 0.07];
const TRACK: [f32; 4] = [1.0, 1.0, 1.0, 0.10];

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() > max {
        text.chars().take(max).collect()
    } else {
        text.to_owned()
    }
}

struct Painter<'a> {
    surface: &'a crate::surface::Hud2dSurface,
    cmds: Vec<Draw2dCmd>,
}

impl Painter<'_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let r = self
            .surface
            .apply_rect(x, y, w, h, ALIGN_RIGHT, ALIGN_TOP);
        self.cmds.push(Draw2dCmd {
            material_namespace: crate::images::HUD_CHROME_NAMESPACE,
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
            s0: 0.0,
            t0: 0.0,
            s1: 1.0,
            t1: 1.0,
            color,
            material: "white".to_owned(),
            op: Draw2dOp::StretchPic,
            provenance: Draw2dProvenance::CgDraw {
                site: "music_panel",
            },
            layer: 1,
        });
    }

    fn text(
        &mut self,
        font: &asset_game::FontDef,
        text: &str,
        x: f32,
        y: f32,
        text_scale: f32,
        color: [f32; 4],
    ) {
        if text.is_empty() {
            return;
        }
        let nscale = hud_iw4::normalized_text_scale(font.pixel_height, text_scale);
        // The draw-text command uses cmd.w/cmd.h as the glyph x/y scale.
        let r = self
            .surface
            .apply_rect(x, y, nscale, nscale, ALIGN_RIGHT, ALIGN_TOP);
        self.cmds.push(Draw2dCmd {
            material_namespace: crate::images::HUD_CHROME_NAMESPACE,
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
            s0: 0.0,
            t0: 0.0,
            s1: 1.0,
            t1: 1.0,
            color,
            material: asset_core::AssetRef::bare_name(&font.material).to_owned(),
            op: Draw2dOp::TextRun {
                font: FONT.to_owned(),
                scale: nscale,
                text: text.to_owned(),
                loc_key: String::new(),
                style: crate::draw2d::TEXT_STYLE_HUDELEM,
                fx: None,
                glow: None,
            },
            provenance: Draw2dProvenance::CgDraw {
                site: "music_panel",
            },
            layer: 1,
        });
    }
}

pub(crate) fn update(
    surface: Res<crate::surface::Hud2dSurface>,
    catalog: Option<Res<MenuCatalog>>,
    player: Option<Res<audio::MusicPlayer>>,
    hud: Option<Res<audio::MusicHudState>>,
    settings: Option<Res<frame::GameSettings>>,
    mut pass: ResMut<HudTessPass>,
    mut gaps: ResMut<HudPresentationGaps>,
    mut hud_images: ResMut<crate::images::HudImages>,
    mut images: ResMut<Assets<Image>>,
) {
    pass.music_panel = TessJob::Hide;
    let (Some(player), Some(hud), Some(settings)) = (player, hud, settings) else {
        return;
    };
    if !hud.visible || !surface.is_ready() {
        return;
    }
    let Some(catalog) = catalog else {
        gaps.raise(GapCause::NoFontCatalog);
        return;
    };
    let Some(font) = catalog.font(FONT) else {
        gaps.raise(GapCause::FontMissing {
            name: FONT.to_owned(),
        });
        return;
    };
    // The panel background is a `white` chrome quad; make sure it exists.
    if hud_images
        .get(crate::images::HUD_CHROME_NAMESPACE, "white", &mut images)
        .is_none()
    {
        gaps.raise(GapCause::NoFontCatalog);
        return;
    }

    let rows = audio::MUSIC_HUD_ROWS;
    let count = player.track_count();
    let current = player.current_index();
    let list_h = ROW_H * rows as f32;
    let panel_h = PAD * 2.0 + HEADER_H + 4.0 + list_h + 4.0 + FOOTER_H;
    let left = -(PANEL_W + PANEL_MARGIN);
    let top = PANEL_MARGIN;

    let mut painter = Painter {
        surface: &surface,
        cmds: Vec::new(),
    };
    painter.rect(left, top, PANEL_W, panel_h, BG);
    // accent frame
    painter.rect(left, top, PANEL_W, 1.2, BORDER);
    painter.rect(left, top, 1.2, panel_h, BORDER);

    let title_scale = 0.30;
    let row_scale = 0.24;
    let small_scale = 0.22;
    painter.text(
        font,
        "MUSIC",
        left + PAD,
        top + PAD,
        title_scale,
        ACCENT,
    );
    let now = player
        .now_playing()
        .map(|name| name.to_uppercase())
        .unwrap_or_else(|| "NOTHING PLAYING".to_owned());
    painter.text(
        font,
        &format!("{} / {}", current + 1, count),
        left + PAD,
        top + PAD,
        small_scale,
        MUTED,
    );

    let list_top = top + PAD + HEADER_H + 4.0;
    for i in 0..rows {
        let index = hud.scroll + i;
        let y = list_top + i as f32 * ROW_H;
        if index >= count {
            continue;
        }
        let selected = hud.browsing && index == hud.selected;
        if selected {
            painter.rect(left, y - 1.0, PANEL_W, ROW_H, ROW_SELECTED);
            painter.rect(left, y - 1.0, 2.0, ROW_H, ACCENT);
        } else if index == current {
            painter.rect(left, y - 1.0, PANEL_W, ROW_H, ROW_CURRENT);
        }
        let name = player.track_name(index).unwrap_or("").to_uppercase();
        let mark = if index == current { '>' } else { ' ' };
        let line = format!("{mark} {:>3} {}", index + 1, truncate(&name, NAME_CHARS));
        let color = if index == current {
            ACCENT
        } else if selected {
            TEXT
        } else {
            MUTED
        };
        painter.text(font, &line, left + PAD, y, row_scale, color);
    }
    // now-playing line under the title
    painter.text(
        font,
        &truncate(&now, NAME_CHARS + 4),
        left + PAD + 44.0,
        top + PAD,
        small_scale,
        TEXT,
    );

    // scrollbar
    let max_scroll = count.saturating_sub(rows);
    if count > rows {
        let track_x = left + PANEL_W - 3.0;
        painter.rect(track_x, list_top, 3.0, list_h, TRACK);
        let ratio = (rows as f32 / count as f32).clamp(0.0, 1.0);
        let thumb_h = (list_h * ratio).max(ROW_H);
        let t = if max_scroll == 0 {
            0.0
        } else {
            hud.scroll as f32 / max_scroll as f32
        };
        painter.rect(
            track_x,
            list_top + (list_h - thumb_h) * t,
            3.0,
            thumb_h,
            ACCENT,
        );
    }

    let footer = format!(
        "MUSIC {:.2}   SOUND {:.2}   {}",
        player.volume,
        settings.master_volume,
        if player.enabled { "PLAYING" } else { "PAUSED" }
    );
    painter.text(
        font,
        &footer,
        left + PAD,
        list_top + list_h + 3.0,
        small_scale,
        MUTED,
    );

    let mut fonts = HashMap::new();
    fonts.insert(FONT.to_owned(), font);
    let (quads, _) = tessellate_fonts(&Draw2dList { cmds: painter.cmds }, &fonts);
    pass.music_panel = TessJob::Quads(quads);
}
