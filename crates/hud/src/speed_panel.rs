//! Speedometer and run timer, drawn just to the right of the minimap while
//! skating, on the game's font pipeline like the other panels.
//!
//! Speed is the board's horizontal speed from [`frame::SkateMode`] (the
//! snapshot's player velocity stays zero while the skate host owns motion); the
//! timer comes from [`frame::RunTimer`].

use std::collections::HashMap;

use asset_game::MenuCatalog;
use bevy::prelude::*;

use crate::draw2d::{Draw2dCmd, Draw2dList, Draw2dOp, Draw2dProvenance, tessellate_fonts};
use crate::gaps::{GapCause, HudPresentationGaps};
use crate::gpu_list::{HudTessPass, TessJob};

#[derive(Component)]
pub(crate) struct SpeedPanelRaster;

const FONT: &str = "fonts/hudsmallfont";
const ALIGN_LEFT: i32 = 1; // hud_iw4::ALIGN_VIEWABLE
const ALIGN_TOP: i32 = 1; // hud_iw4::ALIGN_VIEWABLE

/// Just right of the minimap, which sits in the top-left corner.
const LEFT_X: f32 = 118.0;
const TOP_Y: f32 = 10.0;
const LINE_H: f32 = 15.0;

const SPEED_SCALE: f32 = 0.38;
const VALUE_SCALE: f32 = 0.24;
const LABEL_SCALE: f32 = 0.20;

const ACCENT: [f32; 4] = [0.96, 0.62, 0.14, 1.0];
const TEXT: [f32; 4] = [0.95, 0.96, 0.97, 1.0];
const MUTED: [f32; 4] = [0.68, 0.71, 0.75, 1.0];
const UT_S: f32 = 17.6; // inches per second per mph

fn clock(seconds: f32) -> String {
    let cs = (seconds.max(0.0) * 100.0).round() as i64;
    format!("{}:{:02}.{:02}", cs / 6000, (cs / 100) % 60, cs % 100)
}

struct Painter<'a> {
    surface: &'a crate::surface::Hud2dSurface,
    cmds: Vec<Draw2dCmd>,
}

impl Painter<'_> {
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
            .apply_rect(x, baseline, nscale, nscale, ALIGN_LEFT, ALIGN_TOP);
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
    mode: Option<Res<frame::SkateMode>>,
    timer: Option<Res<frame::RunTimer>>,
    mut pass: ResMut<HudTessPass>,
    mut gaps: ResMut<HudPresentationGaps>,
    mut hud_images: ResMut<crate::images::HudImages>,
    mut images: ResMut<Assets<Image>>,
) {
    pass.speed_panel = TessJob::Hide;
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

    let show_speed = mode.as_ref().is_some_and(|mode| mode.active);
    let speed = mode.as_ref().map_or(0.0, |mode| mode.speed_u_per_s).max(0.0);
    let timer_text = timer.as_ref().and_then(|timer| {
        let show = timer.running || timer.elapsed > 0.0 || timer.best.is_some();
        if !show {
            return None;
        }
        let best = timer
            .best
            .map(clock)
            .unwrap_or_else(|| "--:--.--".to_owned());
        Some(format!("{}   BEST {best}", clock(timer.elapsed)))
    });
    let split_text = timer.as_ref().and_then(|timer| {
        timer.last_split.as_ref().map(|split| {
            let delta = split
                .delta
                .map(|d| format!("{d:+.2}"))
                .unwrap_or_else(|| "   -  ".to_owned());
            format!("{}  {}  {delta}", split.label, clock(split.time))
        })
    });

    if !show_speed && timer_text.is_none() && split_text.is_none() {
        return;
    }

    let mut painter = Painter {
        surface: &surface,
        cmds: Vec::new(),
    };
    let mut y = TOP_Y;
    if show_speed {
        painter.text(font, &format!("{speed:.0} U/S"), LEFT_X, y, SPEED_SCALE, TEXT);
        y += LINE_H + 4.0;
        painter.text(
            font,
            &format!("{:.0} MPH", speed / UT_S),
            LEFT_X,
            y,
            LABEL_SCALE,
            MUTED,
        );
        y += LINE_H;
    }
    if let Some(line) = timer_text.as_deref() {
        painter.text(font, line, LEFT_X, y, VALUE_SCALE, ACCENT);
        y += LINE_H;
    }
    if let Some(line) = split_text.as_deref() {
        painter.text(font, line, LEFT_X, y, LABEL_SCALE, MUTED);
    }

    let mut fonts = HashMap::new();
    fonts.insert(FONT.to_owned(), font);
    let (quads, _) = tessellate_fonts(&Draw2dList { cmds: painter.cmds }, &fonts);
    pass.speed_panel = TessJob::Quads(quads);
}
