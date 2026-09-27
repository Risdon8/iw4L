//! Mod: install the active map layout into a match.
//!
//! Collision and reset rules go into `SimContent`, so the authority and the
//! predicting client share them. Props are extra script models stretched to
//! fill each shape, drawn by the ordinary script-model path.

use assets::SpawnPoint;
use bevy::prelude::*;
use map_layout::Layout;

/// Script-model ids for layout props start here, clear of map entity ordinals.
const LAYOUT_PROP_ID_BASE: u32 = 0x7000_0000;

pub(crate) fn install_sim(content: &mut sim::SimContentBuilder, layout: &Layout) {
    let brushes = layout
        .brushes()
        .into_iter()
        .map(|brush| {
            let n = brush.planes.len();
            sim::SimBrush {
                planes: brush.planes,
                contents: map_layout::CONTENTS_SOLID,
                plane_surface_flags: vec![0; n],
                glass_encoded: 0,
            }
        })
        .collect::<Vec<_>>();
    let rules = sim::LayoutRules {
        reset_below_z: layout.reset.below_z,
        reset_volumes: layout
            .reset
            .volumes
            .iter()
            .map(|volume| (volume.min, volume.max))
            .collect(),
        respawn: layout
            .spawns
            .iter()
            .map(|spawn| (spawn.origin, spawn.yaw))
            .collect(),
        ignore_contents: if layout.ignore_player_clip {
            map_layout::CONTENTS_PLAYER_CLIPS
        } else {
            0
        },
    };
    diag::info!(
        Sim,
        "layout `{}`: {} collision shapes, {} spawns, reset below z {:?}, {} reset volumes, ignore player clip {}",
        layout.name,
        brushes.len(),
        rules.respawn.len(),
        rules.reset_below_z,
        rules.reset_volumes.len(),
        layout.ignore_player_clip
    );
    content.set_layout(brushes, rules);
}

/// The layout's spawns under every free-for-all and team classname, or
/// `None` to keep the map's own.
pub(crate) fn spawn_points(layout: &Layout) -> Option<Vec<SpawnPoint>> {
    if layout.spawns.is_empty() {
        return None;
    }
    let classnames = [gamemode_iw4::ffa::SPAWN_CLASSNAME, "mp_tdm_spawn"];
    Some(
        classnames
            .iter()
            .flat_map(|classname| {
                layout.spawns.iter().map(move |spawn| SpawnPoint {
                    classname: (*classname).to_owned(),
                    origin: spawn.origin,
                    angles: [0.0, spawn.yaw, 0.0],
                    script_linkto: String::new(),
                    script_destructable_area: String::new(),
                })
            })
            .collect(),
    )
}

pub(crate) fn movement_tuning(layout: &Layout) -> Option<sim::MovementTuning> {
    layout.movement.surf.then(|| sim::MovementTuning {
        surf: true,
        ..sim::MovementTuning::default()
    })
}

pub(crate) fn install_props(world: &mut assets::PreparedWorld, layout: &Layout) {
    let mut placed = 0usize;
    for (index, visual) in layout.visuals().into_iter().enumerate() {
        let Some((mins, maxs)) = model_bounds(&world.map_xmodel_scene_assets, &visual.model) else {
            diag::warn!(
                World,
                "layout `{}`: model `{}` is not in this map; shape left invisible",
                layout.name,
                visual.model
            );
            continue;
        };
        let extent = Vec3::from_array(maxs) - Vec3::from_array(mins);
        if extent.min_element() <= 0.01 {
            diag::warn!(
                World,
                "layout `{}`: model `{}` is flat on an axis (bounds {mins:?}..{maxs:?}); cannot stretch it",
                layout.name,
                visual.model
            );
            continue;
        }
        let scale = Vec3::from_array(visual.size) / extent;
        let rotation = Quat::from_mat3(&Mat3::from_cols(
            Vec3::from_array(visual.axes[0]),
            Vec3::from_array(visual.axes[1]),
            Vec3::from_array(visual.axes[2]),
        ));
        let model_mid = (Vec3::from_array(mins) + Vec3::from_array(maxs)) * 0.5;
        let translation = Vec3::from_array(visual.center) - rotation * (scale * model_mid);
        let id = LAYOUT_PROP_ID_BASE + index as u32;
        world
            .script_model_instances
            .push(assets::ScriptModelSceneInstance {
                id: assets::ScriptModelId::from_source_ordinal(id),
                current_model: assets::MapXModelAssetKey(visual.model.clone()),
                transform: Transform {
                    translation,
                    rotation,
                    scale,
                },
                lighting_origin: visual.center,
                dobj_state: assets::dobj::DObjSemanticState::bind_pose(visual.model.clone(), 1, 1),
                metadata: assets::ScriptModelMetadata {
                    targetname: format!("layout_{}", layout.name),
                    ..Default::default()
                },
            });
        placed += 1;
    }
    diag::info!(World, "layout `{}`: {placed} props placed", layout.name);
}

fn model_bounds(
    catalog: &assets::MapXModelSceneCatalog,
    model: &str,
) -> Option<([f32; 3], [f32; 3])> {
    let skel = match catalog.get_name(model)? {
        assets::MapXModelSceneAsset::Iw4(skel)
        | assets::MapXModelSceneAsset::Iw5(skel)
        | assets::MapXModelSceneAsset::T5(skel) => skel,
        assets::MapXModelSceneAsset::Unavailable { .. } => return None,
    };
    let mut points = skel.positions.iter();
    let Some(&first) = points.next() else {
        return skel.bounds;
    };
    Some(points.fold((first, first), |(mut lo, mut hi), p| {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
        (lo, hi)
    }))
}

/// Where the stock spawns sit — a starting point for placing a layout.
pub(crate) fn log_spawn_extent(zone: &str, spawns: &[SpawnPoint]) {
    let Some(first) = spawns.first() else {
        return;
    };
    let (lo, hi) = spawns
        .iter()
        .fold((first.origin, first.origin), |(mut lo, mut hi), s| {
            for i in 0..3 {
                lo[i] = lo[i].min(s.origin[i]);
                hi[i] = hi[i].max(s.origin[i]);
            }
            (lo, hi)
        });
    diag::info!(
        Sim,
        "{zone}: {} stock spawns within x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0}",
        spawns.len(),
        lo[0],
        hi[0],
        lo[1],
        hi[1],
        lo[2],
        hi[2]
    );
}
