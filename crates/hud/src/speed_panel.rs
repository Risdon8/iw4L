//! Speedometer and run timer for the bottom-centre of the HUD, drawn on the
//! game's font pipeline like the other panels.
//!
//! Speed is the local player's horizontal velocity from the presented snapshot;
//! the timer comes from [`frame::RunTimer`].

use std::collections::HashMap;

use asset_game::MenuCatalog;
use bevy::prelude::*;
use net::{LocalPresentClient, PresentedSnapshot};

use crate::draw2d::{Draw2dCmd, Draw2dList, Draw2dOp, Draw2dProvenance, tessellate_fonts};
use crate::gaps::{GapCause, HudPresentationGaps};
use crate::gpu_list::{HudTessPass, TessJob};

#[derive(Component)]
pub(crate) struct SpeedPanelRaster;

const FONT: &str = "fonts/hudsmallfont";
const ALIGN_CENTER: i32 = 2; // hud_iw4::ALIGN_CENTER
const ALIGN_BOTTOM: i32 = 3; // hud_iw4::ALIGN_VIEWABLE_MAX

const BOTTOM_MARGIN: f32 = 46.0;
const LINE_H: f32 = 15.0;

const SPEED_SCALE: f32 = 0.40;
const VALUE_SCALE: f32 = 0.24;
const LABEL_SCALE: f32 = 0.20;

const ACCENT: [f32; 4] = [0.96, 0.62, 0.14, 1.0];
const TEXT: [f32; 4] = [0.95, 0.96, 0.97, 1.0];
const MUTED: [f32; 4] = [0.68, 0.71, 0.75, 1.0];
const UT_S: f32 = 17.6; // inches per second per mph

fn speed_text(u_per_s: f32) -> String {
    format!("{:.0}", u_per_s.max(0.0))
}

fn clock(seconds: f32) -> String {
    let cs = (seconds.max(0.0) * 100.0).round() as i64;
    format!("{}:{:02}.{:02}", cs / 6000, (cs / 100) % 60, cs % 100)
}

struct Painter<'a> {
    surface: &'a crate::surface::Hud2dSurface,
    cmds: Vec<Draw2dCmd>,
}

impl Painter<'_> {
    /// Centred text: `x` is the centre, `y` is measured up from the bottom edge.
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
            .apply_rect(x, baseline, nscale, nscale, ALIGN_CENTER, ALIGN_BOTTOM);
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
                site: "speed_panel",
            },
            layer: 1,
        });
    }
}

pub(crate) fn update(
    surface: Res<crate::surface::Hud2dSurface>,
    catalog: Option<Res<MenuCatalog>>,
    presented: Res<PresentedSnapshot>,
    local: Res<LocalPresentClient>,
    timer: Option<Res<frame::RunTimer>>,
    mut pass: ResMut<HudTessPass>,
    mut gaps: ResMut<HudPresentationGaps>,
    mut hud_images: ResMut<crate::images::HudImages>,
    mut images: ResMut<Assets<Image>>,
) {
    pass.speed_panel = TessJob::Hide;
    let Some(player) = presented.alive_player(local.0) else {
        return;
    };
    if !surface.is_ready() {
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

    let v = player.velocity;
    let speed = (v[0] * v[0] + v[1] * v[1]).sqrt();

    let timer_line = timer.as_ref().map(|timer| {
        let best = timer
            .best
            .map(clock)
            .unwrap_or_else(|| "--:--.--".to_owned());
        let show = timer.running || timer.elapsed > 0.0 || timer.best.is_some();
        if show {
            format!("{}   BEST {best}", clock(timer.elapsed))
        } else {
            String::new()
        }
    });
    let split_line = timer.as_ref().and_then(|timer| {
        timer.last_split.as_ref().map(|split| {
            let delta = split
                .delta
                .map(|d| format!("{d:+.2}"))
                .unwrap_or_else(|| "   -  ".to_owned());
            format!("{}  {}  {delta}", split.label, clock(split.time))
        })
    });

    let lines = 2
        + usize::from(timer_line.as_deref().is_some_and(|l| !l.is_empty()))
        + usize::from(split_line.is_some());
    let total_h = lines as f32 * LINE_H;
    let top = -(BOTTOM_MARGIN + total_h);
    let mut y = top;

    let mut painter = Painter {
        surface: &surface,
        cmds: Vec::new(),
    };
    painter.text(font, &speed_text(speed), 0.0, y, SPEED_SCALE, TEXT);
    y += LINE_H;
    painter.text(
        font,
        &format!("{:.0} MPH", speed / UT_S),
        0.0,
        y,
        LABEL_SCALE,
        MUTED,
    );
    y += LINE_H;
    if let Some(line) = timer_line.as_deref().filter(|l| !l.is_empty()) {
        painter.text(font, line, 0.0, y, VALUE_SCALE, ACCENT);
        y += LINE_H;
    }
    if let Some(line) = split_line.as_deref() {
        painter.text(font, line, 0.0, y, LABEL_SCALE, MUTED);
    }

    let mut fonts = HashMap::new();
    fonts.insert(FONT.to_owned(), font);
    let (quads, _) = tessellate_fonts(&Draw2dList { cmds: painter.cmds }, &fonts);
    pass.speed_panel = TessJob::Quads(quads);
}
