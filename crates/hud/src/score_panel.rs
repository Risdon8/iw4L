//! In-game score / trick panel for skate mode, drawn on the HUD's own font
//! pipeline (the game's `fonts/hudsmallfont` + the `white` chrome image), the
//! same way as the music panel.
//!
//! Data comes from [`frame::SkateMode`], which `render_anim` fills from the
//! Skate host's scoring publication.

use std::collections::HashMap;

use asset_game::MenuCatalog;
use bevy::prelude::*;

use crate::draw2d::{Draw2dCmd, Draw2dList, Draw2dOp, Draw2dProvenance, tessellate_fonts};
use crate::gaps::{GapCause, HudPresentationGaps};
use crate::gpu_list::{HudTessPass, TessJob};

#[derive(Component)]
pub(crate) struct ScorePanelRaster;

const FONT: &str = "fonts/hudsmallfont";
const ALIGN_CENTER: i32 = 2; // hud_iw4::ALIGN_CENTER
const ALIGN_TOP: i32 = 1; // hud_iw4::ALIGN_VIEWABLE

const PANEL_W: f32 = 190.0;
const PANEL_MARGIN: f32 = 10.0;
const PAD: f32 = 8.0;
const LINE_H: f32 = 15.0;
const BAR_H: f32 = 3.0;

const BIG_SCALE: f32 = 0.42;
const VALUE_SCALE: f32 = 0.26;
const LABEL_SCALE: f32 = 0.22;

const BG: [f32; 4] = [0.025, 0.035, 0.05, 0.70];
const BORDER: [f32; 4] = [0.96, 0.62, 0.14, 0.80];
const ACCENT: [f32; 4] = [0.96, 0.62, 0.14, 1.0];
const TEXT: [f32; 4] = [0.93, 0.94, 0.96, 1.0];
const MUTED: [f32; 4] = [0.55, 0.58, 0.62, 1.0];
const FAIL: [f32; 4] = [0.95, 0.30, 0.24, 1.0];
const TRACK: [f32; 4] = [1.0, 1.0, 1.0, 0.14];

fn grouped(value: f32) -> String {
    let digits = (value.max(0.0).round() as i64).to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

struct Painter<'a> {
    surface: &'a crate::surface::Hud2dSurface,
    cmds: Vec<Draw2dCmd>,
}

impl Painter<'_> {
    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        let r = self
            .surface
            .apply_rect(x, y, w, h, ALIGN_CENTER, ALIGN_TOP);
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
                site: "score_panel",
            },
            layer: 1,
        });
    }

    /// Centred text: `x` is the centre of the string in virtual units.
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
        let baseline = y + hud_iw4::ui_text_height(text_scale);
        let r = self
            .surface
            .apply_rect(x, baseline, nscale, nscale, ALIGN_CENTER, ALIGN_TOP);
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
                site: "score_panel",
            },
            layer: 1,
        });
    }
}

pub(crate) fn update(
    surface: Res<crate::surface::Hud2dSurface>,
    catalog: Option<Res<MenuCatalog>>,
    mode: Option<Res<frame::SkateMode>>,
    time: Res<Time>,
    mut pass: ResMut<HudTessPass>,
    mut gaps: ResMut<HudPresentationGaps>,
    mut hud_images: ResMut<crate::images::HudImages>,
    mut images: ResMut<Assets<Image>>,
    mut bail_until: Local<f32>,
) {
    pass.score_panel = TessJob::Hide;
    let Some(mode) = mode else {
        return;
    };
    if !mode.active || !surface.is_ready() {
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
    if hud_images
        .get(crate::images::HUD_CHROME_NAMESPACE, "white", &mut images)
        .is_none()
    {
        gaps.raise(GapCause::NoFontCatalog);
        return;
    }

    let now = time.elapsed_secs();
    if mode.score_bailed {
        *bail_until = now + 1.4;
    }
    let bailed = now < *bail_until;

    let active = mode.trick_active && mode.score_sequence > 0.0;
    let rows = if active || bailed { 4.0 } else { 2.0 };
    let panel_h = PAD * 2.0 + LINE_H * rows + BAR_H + 4.0;
    let top = PANEL_MARGIN;

    let mut painter = Painter {
        surface: &surface,
        cmds: Vec::new(),
    };
    painter.rect(-PANEL_W * 0.5, top, PANEL_W, panel_h, BG);
    painter.rect(-PANEL_W * 0.5, top, PANEL_W, 1.2, BORDER);
    painter.rect(-PANEL_W * 0.5, top + panel_h - 1.2, PANEL_W, 1.2, BORDER);
    painter.rect(-PANEL_W * 0.5, top, 1.2, panel_h, BORDER);
    painter.rect(PANEL_W * 0.5 - 1.2, top, 1.2, panel_h, BORDER);

    let mut y = top + PAD;
    painter.text(font, "SCORE", 0.0, y, LABEL_SCALE, MUTED);
    y += LINE_H;
    painter.text(font, &grouped(mode.score_total), 0.0, y, BIG_SCALE, TEXT);
    y += LINE_H;

    if active || bailed {
        let combo = if bailed {
            "BAILED".to_owned()
        } else {
            format!("x{:.1}   {}", mode.score_multiplier, grouped(mode.score_sequence))
        };
        let combo_color = if bailed { FAIL } else { ACCENT };
        painter.text(font, &combo, 0.0, y, VALUE_SCALE, combo_color);
        y += LINE_H;
        let trick = if mode.trick.is_empty() {
            "-".to_owned()
        } else {
            mode.trick.to_uppercase()
        };
        painter.text(font, &trick, 0.0, y, LABEL_SCALE, MUTED);
        y += LINE_H;

        painter.rect(
            -PANEL_W * 0.5 + PAD,
            y,
            PANEL_W - PAD * 2.0,
            BAR_H,
            TRACK,
        );
        if active {
            let frac = mode.score_combo_fraction.clamp(0.0, 1.0);
            painter.rect(
                -PANEL_W * 0.5 + PAD,
                y,
                (PANEL_W - PAD * 2.0) * frac,
                BAR_H,
                ACCENT,
            );
        }
    }

    let mut fonts = HashMap::new();
    fonts.insert(FONT.to_owned(), font);
    let (quads, _) = tessellate_fonts(&Draw2dList { cmds: painter.cmds }, &fonts);
    pass.score_panel = TessJob::Quads(quads);
}
