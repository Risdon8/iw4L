//! In-game score / trick readout for skate mode, drawn on the HUD's own font
//! pipeline (the game's `fonts/hudsmallfont`), the same way as the music panel.
//!
//! Data comes from [`frame::SkateMode`], which `render_anim` fills from the
//! Skate host's scoring publication. This adds the presentation layer: a
//! session best, a BANKED confirmation when a combo lands, and trick-name
//! popups.

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

const MARGIN: f32 = 12.0;
const LINE_H: f32 = 16.0;
const BAR_H: f32 = 3.0;
const BAR_W: f32 = 150.0;
const POPUP_H: f32 = 13.0;
const POPUP_LIFE: f32 = 2.2;
const BANK_HOLD: f32 = 1.6;
const BAIL_HOLD: f32 = 1.4;

const BIG_SCALE: f32 = 0.46;
const VALUE_SCALE: f32 = 0.26;
const LABEL_SCALE: f32 = 0.22;

const ACCENT: [f32; 4] = [0.96, 0.62, 0.14, 1.0];
const TEXT: [f32; 4] = [0.95, 0.96, 0.97, 1.0];
const MUTED: [f32; 4] = [0.68, 0.71, 0.75, 1.0];
const FAIL: [f32; 4] = [0.98, 0.34, 0.28, 1.0];

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

#[derive(Default)]
pub(crate) struct ScoreFx {
    best: f32,
    prev_total: f32,
    prev_trick: String,
    bank: Option<(f32, f32)>,
    bail_until: f32,
    popups: Vec<(String, f32)>,
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

fn fade(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], color[3] * alpha.clamp(0.0, 1.0)]
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
    mut fx: Local<ScoreFx>,
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
        fx.bail_until = now + BAIL_HOLD;
    }
    let bailed = now < fx.bail_until;

    // Session best.
    if mode.score_total > fx.best {
        fx.best = mode.score_total;
    }
    // A successful publish moves the total; a bail does not.
    if mode.score_total > fx.prev_total + 0.5 {
        fx.bank = Some((mode.score_total - fx.prev_total, now + BANK_HOLD));
    }
    fx.prev_total = mode.score_total;

    // Trick-name popups: one per announced trick.
    if mode.trick_active && !mode.trick.is_empty() && mode.trick != fx.prev_trick {
        fx.popups.push((mode.trick.clone(), now));
        fx.prev_trick = mode.trick.clone();
        while fx.popups.len() > 4 {
            fx.popups.remove(0);
        }
    }
    if !mode.trick_active {
        fx.prev_trick.clear();
    }
    fx.popups.retain(|(_, born)| now - *born < POPUP_LIFE);

    let active = mode.trick_active && mode.score_sequence > 0.0;

    let mut painter = Painter {
        surface: &surface,
        cmds: Vec::new(),
    };
    let mut y = MARGIN;

    painter.text(font, &grouped(mode.score_total), 0.0, y, BIG_SCALE, TEXT);
    y += LINE_H;
    painter.text(
        font,
        &format!("BEST {}", grouped(fx.best)),
        0.0,
        y,
        LABEL_SCALE,
        MUTED,
    );
    y += LINE_H;

    if active || bailed {
        let (combo, color) = if bailed {
            ("BAILED".to_owned(), FAIL)
        } else {
            (
                format!(
                    "x{:.1}   {}",
                    mode.score_multiplier,
                    grouped(mode.score_sequence)
                ),
                ACCENT,
            )
        };
        painter.text(font, &combo, 0.0, y, VALUE_SCALE, color);
        y += LINE_H;
        let trick = if mode.trick.is_empty() {
            "-".to_owned()
        } else {
            mode.trick.to_uppercase()
        };
        painter.text(font, &trick, 0.0, y, LABEL_SCALE, MUTED);
        y += LINE_H - 4.0;
        if active {
            let frac = mode.score_combo_fraction.clamp(0.0, 1.0);
            painter.rect(-BAR_W * 0.5, y, BAR_W * frac, BAR_H, ACCENT);
        }
        y += LINE_H;
    }

    if let Some((amount, until)) = fx.bank
        && now < until
    {
        let alpha = ((until - now) / BANK_HOLD).clamp(0.0, 1.0);
        painter.text(
            font,
            &format!("BANKED +{}", grouped(amount)),
            0.0,
            y,
            VALUE_SCALE,
            fade(ACCENT, alpha),
        );
        y += LINE_H;
    }

    for (name, born) in fx.popups.iter().rev() {
        let age = (now - *born).max(0.0);
        let alpha = (1.0 - age / POPUP_LIFE).clamp(0.0, 1.0);
        painter.text(
            font,
            &name.to_uppercase(),
            0.0,
            y,
            LABEL_SCALE,
            fade(TEXT, alpha),
        );
        y += POPUP_H;
    }

    let mut fonts = HashMap::new();
    fonts.insert(FONT.to_owned(), font);
    let (quads, _) = tessellate_fonts(&Draw2dList { cmds: painter.cmds }, &fonts);
    pass.score_panel = TessJob::Quads(quads);
}
