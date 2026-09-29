//! Speedometer, drawn just to the right of the minimap while skating, on the
//! game's font pipeline like the other panels.
//!
//! Speed is the board's horizontal speed from [`frame::SkateMode`] (the
//! snapshot's player velocity stays zero while the skate host owns motion).

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
const SPEED_SCALE: f32 = 0.38;

const TEXT: [f32; 4] = [0.95, 0.96, 0.97, 1.0];
const UT_S: f32 = 17.6; // inches per second per mph

pub(crate) fn update(
    surface: Res<crate::surface::Hud2dSurface>,
    catalog: Option<Res<MenuCatalog>>,
    mode: Option<Res<frame::SkateMode>>,
    mut pass: ResMut<HudTessPass>,
    mut gaps: ResMut<HudPresentationGaps>,
    mut hud_images: ResMut<crate::images::HudImages>,
    mut images: ResMut<Assets<Image>>,
) {
    pass.speed_panel = TessJob::Hide;
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

    let mph = mode.speed_u_per_s.max(0.0) / UT_S;
    let text = format!("{mph:.0} MPH");
    let nscale = hud_iw4::normalized_text_scale(font.pixel_height, SPEED_SCALE);
    let baseline = TOP_Y + hud_iw4::ui_text_height(SPEED_SCALE);
    let rect = surface.apply_rect(LEFT_X, baseline, nscale, nscale, ALIGN_LEFT, ALIGN_TOP);

    let mut cmds = vec![Draw2dCmd {
        material_namespace: crate::images::HUD_CHROME_NAMESPACE,
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        s0: 0.0,
        t0: 0.0,
        s1: 1.0,
        t1: 1.0,
        color: TEXT,
        material: asset_core::AssetRef::bare_name(&font.material).to_owned(),
        op: Draw2dOp::TextRun {
            font: FONT.to_owned(),
            scale: nscale,
            text,
            loc_key: String::new(),
            style: crate::draw2d::TEXT_STYLE_HUDELEM,
            fx: None,
            glow: None,
        },
        provenance: Draw2dProvenance::CgDraw {
            site: "speed_panel",
        },
        layer: 1,
    }];

    let mut fonts = HashMap::new();
    fonts.insert(FONT.to_owned(), font);
    let (quads, _) = tessellate_fonts(&Draw2dList { cmds }, &fonts);
    pass.speed_panel = TessJob::Quads(quads);
}
