use bevy::prelude::*;

use super::dof::GlowDvars;

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct FilmVisionView {
    pub current: Option<asset_world::FilmVision>,
    from: hud_iw4::VisionSetVars,
    to: hud_iw4::VisionSetVars,
    result: hud_iw4::VisionSetVars,
    lerp: hud_iw4::VisionSetLerpData,
    last_map: Option<asset_world::FilmVision>,
    override_vision: Option<asset_world::FilmVision>,
}

impl Default for FilmVisionView {
    fn default() -> Self {
        Self {
            current: None,
            from: hud_iw4::VisionSetVars::default(),
            to: hud_iw4::VisionSetVars::default(),
            result: hud_iw4::VisionSetVars::default(),
            lerp: hud_iw4::VisionSetLerpData::default(),
            last_map: None,
            override_vision: None,
        }
    }
}

impl FilmVisionView {
    /// Select a preset, or restore the map with None. Call only for a ready view.
    pub fn select(
        &mut self,
        map: Option<asset_world::FilmVision>,
        preset: Option<asset_world::FilmVision>,
        now_ms: i32,
        duration_ms: i32,
        allowed: bool,
        script_forced: bool,
    ) {
        if self.last_map.is_none() {
            self.result = pack_film_vision(map.unwrap_or_default());
            self.lerp.style = hud_iw4::VISION_SET_LERP_HOLD;
        }
        let (current, _) = hud_iw4::vision_sets_update(
            now_ms,
            self.from,
            self.to,
            self.lerp,
            self.result,
            allowed,
            script_forced,
        );
        let target = preset.or(map).unwrap_or_default();
        (self.from, self.to, self.lerp) = hud_iw4::vision_set_start(
            now_ms,
            duration_ms,
            hud_iw4::VISION_SET_LERP_TO_SMOOTH,
            self.lerp.style,
            current,
            pack_film_vision(target),
        );
        self.result = if duration_ms <= 0 { self.to } else { current };
        self.last_map = Some(target);
        self.override_vision = preset;
    }
}

pub fn pack_film_vision(vision: asset_world::FilmVision) -> hud_iw4::VisionSetVars {
    hud_iw4::VisionSetVars {
        r_glow: vision.glow_enable,
        r_glow_bloom_cutoff: vision.glow_bloom_cutoff,
        r_glow_bloom_desaturation: vision.glow_bloom_desaturation,
        r_glow_bloom_intensity0: vision.glow_bloom_intensity,
        r_glow_radius0: vision.glow_radius,
        r_film_enable: vision.enable,
        r_film_brightness: vision.brightness,
        r_film_contrast: vision.contrast,
        r_film_desaturation: vision.desaturation,
        r_film_desaturation_dark: vision.desaturation_dark,
        r_film_invert: vision.invert,
        r_film_light_tint: vision.light_tint,
        r_film_medium_tint: vision.medium_tint,
        r_film_dark_tint: vision.dark_tint,
        ..hud_iw4::VisionSetVars::default()
    }
}

pub fn unpack_film_vision(vars: hud_iw4::VisionSetVars) -> asset_world::FilmVision {
    asset_world::FilmVision {
        enable: vars.r_film_enable,
        contrast: vars.r_film_contrast,
        brightness: vars.r_film_brightness,
        desaturation: vars.r_film_desaturation,
        desaturation_dark: vars.r_film_desaturation_dark,
        invert: vars.r_film_invert,
        light_tint: vars.r_film_light_tint,
        medium_tint: vars.r_film_medium_tint,
        dark_tint: vars.r_film_dark_tint,
        glow_enable: vars.r_glow,
        glow_radius: vars.r_glow_radius0,
        glow_bloom_cutoff: vars.r_glow_bloom_cutoff,
        glow_bloom_desaturation: vars.r_glow_bloom_desaturation,
        glow_bloom_intensity: vars.presented_glow_intensity0(),
    }
}

#[must_use]
pub fn presented_film_vision(
    view_ready: bool,
    map_vision: Option<asset_world::FilmVision>,
) -> Option<asset_world::FilmVision> {
    if view_ready { map_vision } else { None }
}

#[must_use]
pub fn presented_film_vision_with_glow_tweaks(
    view_ready: bool,
    map_vision: Option<asset_world::FilmVision>,
    use_tweaks: bool,
    tweaks: lighting_iw4::GlowViewInfo,
) -> Option<asset_world::FilmVision> {
    if !view_ready {
        return None;
    }
    if !use_tweaks {
        return map_vision;
    }
    let mut vision = map_vision.unwrap_or_default();
    let selected = lighting_iw4::select_glow_view_info(
        lighting_iw4::GlowViewInfo {
            enable: vision.glow_enable,
            cutoff: vision.glow_bloom_cutoff,
            desaturation: vision.glow_bloom_desaturation,
            intensity: vision.glow_bloom_intensity,
            radius: vision.glow_radius,
        },
        true,
        tweaks,
    );
    vision.glow_enable = selected.enable;
    vision.glow_bloom_cutoff = selected.cutoff;
    vision.glow_bloom_desaturation = selected.desaturation;
    vision.glow_bloom_intensity = selected.intensity;
    vision.glow_radius = selected.radius;
    Some(vision)
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn presented_film_vision_with_lerp(
    view_ready: bool,
    map_vision: Option<asset_world::FilmVision>,
    now_ms: i32,
    duration_ms: i32,
    slot: &mut FilmVisionView,
    use_tweaks: bool,
    tweaks: lighting_iw4::GlowViewInfo,
    allowed: bool,
    script_forced: bool,
) -> Option<asset_world::FilmVision> {
    if !view_ready {
        *slot = FilmVisionView::default();
        return None;
    }
    let Some(map) = map_vision else {
        *slot = FilmVisionView::default();
        return None;
    };
    if slot.last_map != Some(map) {
        let (from, to, lerp) = hud_iw4::vision_set_start(
            now_ms,
            duration_ms,
            hud_iw4::VISION_SET_LERP_TO_LINEAR,
            slot.lerp.style,
            slot.result,
            pack_film_vision(map),
        );
        slot.from = from;
        slot.to = to;
        slot.lerp = lerp;
        slot.last_map = Some(map);
        if slot.lerp.style == hud_iw4::VISION_SET_LERP_HOLD {
            slot.result = slot.to;
        }
    }
    let (vars, lerp) = hud_iw4::vision_sets_update(
        now_ms,
        slot.from,
        slot.to,
        slot.lerp,
        slot.result,
        allowed,
        script_forced,
    );
    slot.lerp = lerp;
    slot.result = vars;
    let mixed = unpack_film_vision(vars);
    presented_film_vision_with_glow_tweaks(true, Some(mixed), use_tweaks, tweaks)
}

#[derive(Resource, Default)]
struct AppliedVision(Option<Option<sim::VisionChange>>);

pub fn register(app: &mut App) {
    app.init_resource::<AppliedVision>();
    app.init_resource::<FilmVisionView>().add_systems(
        Update,
        update_film_vision_view
            .after(crate::prepare::scene::view_parms::stamp_prepared_scene_view)
            .in_set(net::ClientSet::Present),
    );
}

#[allow(clippy::too_many_arguments)]
fn update_film_vision_view(
    view: Res<crate::prepare::scene::view_parms::PreparedSceneView>,
    scene: Res<crate::prepare::scene::world::WorldScene>,
    glow: Res<GlowDvars>,
    clock: Res<net::FrameClock>,
    mut film: ResMut<FilmVisionView>,
    presented: Res<net::PresentedSnapshot>,
    local: Res<net::LocalPresentClient>,
    mut applied: ResMut<AppliedVision>,
    settings: Res<frame::GameSettings>,
) {
    if !view.ready {
        applied.0 = None;
    } else {
        let wanted = presented.snapshot().and_then(|snapshot| {
            let meta = snapshot.meta.for_client(local.0)?;
            let effects = &meta.view_effects;
            let global = &snapshot.meta.objectives;
            if meta.remote_missile.is_some() {
                effects
                    .missile_vision
                    .clone()
                    .or_else(|| global.missile_vision.clone())
            } else if presented
                .player(local.0)
                .is_some_and(|ps| ps.other_flags & 0x8 != 0)
            {
                effects
                    .thermal_vision
                    .clone()
                    .or_else(|| global.thermal_vision.clone())
            } else {
                effects
                    .naked_vision
                    .clone()
                    .or_else(|| global.naked_vision.clone())
            }
        });
        if applied.0.as_ref() != Some(&wanted) {
            let preset = wanted.as_ref().and_then(|vision| {
                let key = format!("vision/{}.vision", vision.name.to_ascii_lowercase());
                match scene.film_visions.get(&key) {
                    Some(Ok(preset)) => Some(*preset),
                    Some(Err(error)) => {
                        diag::warn!(World, "vision {key}: {error:?}");
                        None
                    }
                    None => {
                        diag::warn!(World, "vision {key} is not loaded");
                        None
                    }
                }
            });
            let duration_ms = match (&applied.0, &wanted) {
                (Some(_), Some(vision)) => vision.duration_ms,
                _ => 0,
            };
            film.select(
                scene.film_vision,
                preset,
                clock.time(),
                duration_ms,
                glow.allowed,
                glow.allowed_script_forced,
            );
            applied.0 = Some(wanted);
        }
    }
    let mixed = presented_film_vision_with_lerp(
        view.ready,
        film.override_vision.or(scene.film_vision),
        clock.time(),
        0,
        &mut film,
        glow.use_tweaks,
        glow.tweak_view_info(),
        glow.allowed,
        glow.allowed_script_forced,
    );
    film.current = if view.ready && settings.brightness != 0.0 {
        let mut vision = mixed.unwrap_or(asset_world::FilmVision::default());
        vision.enable = true;
        vision.brightness += settings.brightness;
        Some(vision)
    } else {
        mixed
    };
}
