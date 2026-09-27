//! Mod: map layouts — a JSON file applied on top of a stock map.
//!
//! A layout adds convex collision shapes (boxes and surf ramps), dresses each
//! one with a stretched prop from the base map, overrides the spawn points,
//! resets players who fall out of the course and sets movement defaults.
//! Geometry here is plain arrays; `sim` and `session` convert it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::Deserialize;

pub const CONTENTS_SOLID: u32 = 1;

/// The two invisible clip bits in the player trace mask (everything in it
/// except solid, glass and bodies).
pub const CONTENTS_PLAYER_CLIPS: u32 = 0x0081_0000;

const DEFAULT_MODEL: &str = "ch_crate64x64";
const DEFAULT_RAMP_ANGLE: f32 = 60.0;
const RAMP_VISUAL_THICKNESS: f32 = 8.0;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub name: String,
    pub base_map: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub movement: Movement,
    #[serde(default)]
    pub spawns: Vec<Spawn>,
    #[serde(default)]
    pub reset: Reset,
    /// Let players pass the base map's invisible out-of-bounds clip, so a
    /// course can sit in open air beyond the playable area.
    #[serde(default)]
    pub ignore_player_clip: bool,
    #[serde(default = "default_model")]
    pub default_model: String,
    #[serde(default)]
    pub shapes: Vec<Shape>,
    /// Ordered sections of a course. Standing in a checkpoint's volume makes
    /// it where a fall sends you; `checkpoint` jumps straight to one.
    #[serde(default)]
    pub checkpoints: Vec<Checkpoint>,
}

fn default_model() -> String {
    DEFAULT_MODEL.to_owned()
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Movement {
    #[serde(default)]
    pub surf: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spawn {
    pub origin: [f32; 3],
    #[serde(default)]
    pub yaw: f32,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reset {
    /// Anyone whose origin drops below this height goes back to a spawn.
    #[serde(default)]
    pub below_z: Option<f32>,
    #[serde(default)]
    pub volumes: Vec<Aabb>,
    /// Like `volumes`, but always sends the player back to the start even
    /// after a checkpoint: the course's "go again" portal.
    #[serde(default)]
    pub restart_volumes: Vec<Aabb>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// One ordered section of a course: standing inside `volume` remembers it, so a
/// later fall returns the player to `origin`. `origin` must lie in `volume`,
/// otherwise the checkpoint cannot be held by standing on it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    #[serde(default)]
    pub name: String,
    pub volume: Aabb,
    pub origin: [f32; 3],
    #[serde(default)]
    pub yaw: f32,
}

impl Aabb {
    pub fn contains(&self, p: [f32; 3]) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    /// An oriented box. `angles` is `[pitch, yaw, roll]` in degrees, the
    /// same convention as map entities.
    Box {
        #[serde(default)]
        name: String,
        center: [f32; 3],
        size: [f32; 3],
        #[serde(default)]
        angles: [f32; 3],
        #[serde(default)]
        model: Option<String>,
        #[serde(default = "default_true")]
        visible: bool,
    },
    /// A surf ramp: a triangular prism lying along `yaw`, `center` at the
    /// middle of its base. The two faces are `angle` degrees from flat
    /// (default 60, too steep to stand on) unless `height` is given. `drop`
    /// tilts the whole ramp so its far end is that much lower: gravity then
    /// pulls a surfer along it.
    Ramp {
        #[serde(default)]
        name: String,
        center: [f32; 3],
        length: f32,
        width: f32,
        #[serde(default)]
        height: Option<f32>,
        #[serde(default)]
        angle: Option<f32>,
        #[serde(default)]
        yaw: f32,
        #[serde(default)]
        drop: f32,
        #[serde(default)]
        model: Option<String>,
        #[serde(default = "default_true")]
        visible: bool,
    },
}

/// A convex brush: outward planes `[nx, ny, nz, dist]`, inside where
/// `n·p <= dist`.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    pub planes: Vec<[f32; 4]>,
}

/// An oriented box region a prop is stretched to fill. `axes` are the unit
/// local X, Y, Z directions in world space (right-handed).
#[derive(Clone, Debug, PartialEq)]
pub struct Visual {
    pub model: String,
    pub center: [f32; 3],
    pub axes: [[f32; 3]; 3],
    pub size: [f32; 3],
}

impl Layout {
    pub fn parse(text: &str) -> Result<Self, String> {
        let layout: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        layout.validate()?;
        Ok(layout)
    }

    pub fn applies_to_zone(&self, zone: &str) -> bool {
        let stem = zone.rsplit(':').next().unwrap_or(zone);
        stem.eq_ignore_ascii_case(&self.base_map)
    }

    /// Resolves a `checkpoint` argument: a 0-based index, or a name
    /// (case-insensitive). An index wins when both would match.
    pub fn checkpoint_index(&self, key: &str) -> Option<usize> {
        if let Ok(index) = key.parse::<usize>()
            && index < self.checkpoints.len()
        {
            return Some(index);
        }
        self.checkpoints
            .iter()
            .position(|checkpoint| checkpoint.name.eq_ignore_ascii_case(key))
    }

    pub fn brushes(&self) -> Vec<Brush> {
        self.shapes.iter().map(Shape::brush).collect()
    }

    pub fn visuals(&self) -> Vec<Visual> {
        self.shapes
            .iter()
            .flat_map(|shape| shape.visuals(&self.default_model))
            .collect()
    }

    fn validate(&self) -> Result<(), String> {
        if self.base_map.trim().is_empty() {
            return Err("base_map is empty".into());
        }
        let finite = |v: &[f32]| v.iter().all(|x| x.is_finite());
        for spawn in &self.spawns {
            if !finite(&spawn.origin) || !spawn.yaw.is_finite() {
                return Err("spawn has a non-finite value".into());
            }
        }
        for (index, checkpoint) in self.checkpoints.iter().enumerate() {
            let label = if checkpoint.name.is_empty() {
                format!("checkpoint #{index}")
            } else {
                format!("checkpoint `{}`", checkpoint.name)
            };
            if !finite(&checkpoint.origin) || !checkpoint.yaw.is_finite() {
                return Err(format!("{label} has a non-finite value"));
            }
            if !finite(&checkpoint.volume.min) || !finite(&checkpoint.volume.max) {
                return Err(format!("{label} has a non-finite volume"));
            }
            if (0..3).any(|i| checkpoint.volume.min[i] > checkpoint.volume.max[i]) {
                return Err(format!("{label} volume min is above its max"));
            }
            if !checkpoint.volume.contains(checkpoint.origin) {
                return Err(format!("{label} origin is outside its volume"));
            }
        }
        for (index, volume) in self
            .reset
            .volumes
            .iter()
            .chain(&self.reset.restart_volumes)
            .enumerate()
        {
            if !finite(&volume.min) || !finite(&volume.max) {
                return Err(format!("reset volume #{index} has a non-finite bound"));
            }
            if (0..3).any(|i| volume.min[i] > volume.max[i]) {
                return Err(format!("reset volume #{index} min is above its max"));
            }
        }
        for (index, shape) in self.shapes.iter().enumerate() {
            let label = |name: &str| {
                if name.is_empty() {
                    format!("shape #{index}")
                } else {
                    format!("shape `{name}`")
                }
            };
            match shape {
                Shape::Box {
                    name,
                    center,
                    size,
                    angles,
                    ..
                } => {
                    if !finite(center) || !finite(angles) || !finite(size) {
                        return Err(format!("{} has a non-finite value", label(name)));
                    }
                    if size.iter().any(|s| *s <= 0.0) {
                        return Err(format!("{}: size must be positive", label(name)));
                    }
                }
                Shape::Ramp {
                    name,
                    center,
                    length,
                    width,
                    height,
                    angle,
                    yaw,
                    drop,
                    ..
                } => {
                    if !finite(center) || !finite(&[*length, *width, *yaw, *drop]) {
                        return Err(format!("{} has a non-finite value", label(name)));
                    }
                    if *length <= 0.0 || *width <= 0.0 {
                        return Err(format!(
                            "{}: length and width must be positive",
                            label(name)
                        ));
                    }
                    if drop.abs() >= *length {
                        return Err(format!("{}: drop must be smaller than length", label(name)));
                    }
                    if height.is_some() && angle.is_some() {
                        return Err(format!("{}: give height or angle, not both", label(name)));
                    }
                    if height.is_some_and(|h| !h.is_finite() || h <= 0.0) {
                        return Err(format!("{}: height must be positive", label(name)));
                    }
                    if angle.is_some_and(|a| !a.is_finite() || a <= 0.0 || a >= 90.0) {
                        return Err(format!("{}: angle must be between 0 and 90", label(name)));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Shape {
    fn brush(&self) -> Brush {
        match self {
            Shape::Box {
                center,
                size,
                angles,
                ..
            } => box_brush(*center, rotation(*angles), *size),
            Shape::Ramp { center, .. } => {
                let Some(r) = self.ramp_frame() else {
                    return Brush { planes: Vec::new() };
                };
                let mut planes = vec![
                    plane(neg(r.up), center),
                    plane(r.forward, &add(*center, scale(r.forward, r.half_length))),
                    plane(
                        neg(r.forward),
                        &add(*center, scale(r.forward, -r.half_length)),
                    ),
                ];
                for face in r.faces() {
                    planes.push(plane(face.normal, &face.base_edge_mid));
                }
                Brush { planes }
            }
        }
    }

    fn ramp_frame(&self) -> Option<RampFrame> {
        let Shape::Ramp {
            center,
            length,
            width,
            yaw,
            drop,
            ..
        } = self
        else {
            return None;
        };
        let (s, c) = yaw.to_radians().sin_cos();
        let tilt = drop.atan2(*length);
        let (sp, cp) = tilt.sin_cos();
        let forward = [c * cp, s * cp, -sp];
        let left = [-s, c, 0.0];
        Some(RampFrame {
            center: *center,
            forward,
            left,
            up: cross(forward, left),
            half_length: (length * length + drop * drop).sqrt() * 0.5,
            half_width: width * 0.5,
            height: self.ramp_height(),
        })
    }

    fn visuals(&self, default_model: &str) -> Vec<Visual> {
        match self {
            Shape::Box {
                center,
                size,
                angles,
                model,
                visible,
                ..
            } => {
                if !visible {
                    return Vec::new();
                }
                vec![Visual {
                    model: model.clone().unwrap_or_else(|| default_model.to_owned()),
                    center: *center,
                    axes: rotation(*angles),
                    size: *size,
                }]
            }
            Shape::Ramp { model, visible, .. } => {
                let Some(r) = self.ramp_frame().filter(|_| *visible) else {
                    return Vec::new();
                };
                let model = model.clone().unwrap_or_else(|| default_model.to_owned());
                r.faces()
                    .into_iter()
                    .map(|face| {
                        let z = face.normal;
                        let x = r.forward;
                        let y = cross(z, x);
                        Visual {
                            model: model.clone(),
                            center: add(face.mid, scale(z, -RAMP_VISUAL_THICKNESS * 0.5)),
                            axes: [x, y, z],
                            size: [r.half_length * 2.0, face.slant, RAMP_VISUAL_THICKNESS],
                        }
                    })
                    .collect()
            }
        }
    }

    fn ramp_height(&self) -> f32 {
        match self {
            Shape::Ramp {
                width,
                height,
                angle,
                ..
            } => height.unwrap_or_else(|| {
                width * 0.5 * angle.unwrap_or(DEFAULT_RAMP_ANGLE).to_radians().tan()
            }),
            Shape::Box { size, .. } => size[2],
        }
    }
}

/// `forward` runs down the ramp's length (tilted by `drop`), `left` is
/// horizontal and `up` completes the right-handed frame.
struct RampFrame {
    center: [f32; 3],
    forward: [f32; 3],
    left: [f32; 3],
    up: [f32; 3],
    half_length: f32,
    half_width: f32,
    height: f32,
}

struct RampFace {
    normal: [f32; 3],
    base_edge_mid: [f32; 3],
    mid: [f32; 3],
    slant: f32,
}

impl RampFrame {
    fn faces(&self) -> [RampFace; 2] {
        let up = self.up;
        let apex = add(self.center, scale(up, self.height));
        let slant = (self.half_width * self.half_width + self.height * self.height).sqrt();
        [1.0_f32, -1.0].map(|side| {
            let lateral = scale(self.left, side);
            let normal = normalize(add(scale(lateral, self.height), scale(up, self.half_width)));
            let base_edge_mid = add(self.center, scale(lateral, self.half_width));
            RampFace {
                normal,
                base_edge_mid,
                mid: scale(add(base_edge_mid, apex), 0.5),
                slant,
            }
        })
    }
}

fn box_brush(center: [f32; 3], axes: [[f32; 3]; 3], size: [f32; 3]) -> Brush {
    let mut planes = Vec::with_capacity(6);
    for (axis, extent) in axes.iter().zip(size) {
        let half = extent * 0.5;
        planes.push(plane(*axis, &add(center, scale(*axis, half))));
        planes.push(plane(neg(*axis), &add(center, scale(*axis, -half))));
    }
    Brush { planes }
}

fn plane(normal: [f32; 3], point: &[f32; 3]) -> [f32; 4] {
    [normal[0], normal[1], normal[2], dot(normal, *point)]
}

/// Local X, Y, Z axes for `[pitch, yaw, roll]` degrees: yaw about Z, then
/// pitch about Y, then roll about X — the order map entities use.
pub fn rotation(angles: [f32; 3]) -> [[f32; 3]; 3] {
    let (sp, cp) = angles[0].to_radians().sin_cos();
    let (sy, cy) = angles[1].to_radians().sin_cos();
    let (sr, cr) = angles[2].to_radians().sin_cos();
    [
        [cy * cp, sy * cp, -sp],
        [cy * sp * sr - sy * cr, sy * sp * sr + cy * cr, cp * sr],
        [cy * sp * cr + sy * sr, sy * sp * cr - cy * sr, cp * cr],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn neg(a: [f32; 3]) -> [f32; 3] {
    scale(a, -1.0)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(a: [f32; 3]) -> [f32; 3] {
    let len = dot(a, a).sqrt();
    if len > 0.0 { scale(a, 1.0 / len) } else { a }
}

static ACTIVE: RwLock<Option<Arc<Layout>>> = RwLock::new(None);
static ENV_INIT: std::sync::Once = std::sync::Once::new();

/// `IW4L_LAYOUT` is read on first use rather than at startup, so a value
/// from `.env` (loaded before the first map) still counts.
fn init_once() {
    ENV_INIT.call_once(|| {
        if let Some(Ok(layout)) = env_layout() {
            store(Some(layout));
        }
    });
}

fn store(layout: Option<Layout>) {
    let mut slot = ACTIVE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = layout.map(Arc::new);
}

pub fn set_active(layout: Option<Layout>) {
    init_once();
    store(layout);
}

pub fn active() -> Option<Arc<Layout>> {
    init_once();
    ACTIVE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// The active layout if it was made for `zone`.
pub fn active_for_zone(zone: &str) -> Option<Arc<Layout>> {
    active().filter(|layout| layout.applies_to_zone(zone))
}

/// `IW4L_LAYOUT_DIR`, else the nearest `layouts/` folder above the
/// executable or the working directory.
pub fn layouts_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("IW4L_LAYOUT_DIR") {
        return Some(PathBuf::from(dir));
    }
    let starts = [
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        std::env::current_dir().ok(),
    ];
    starts.into_iter().flatten().find_map(|start| {
        start
            .ancestors()
            .map(|dir| dir.join("layouts"))
            .find(|dir| dir.is_dir())
    })
}

pub fn load(name: &str) -> Result<Layout, String> {
    let path = Path::new(name);
    let path = if path.extension().is_some() || path.components().count() > 1 {
        path.to_path_buf()
    } else {
        layouts_dir()
            .ok_or("no layouts/ folder found (set IW4L_LAYOUT_DIR)")?
            .join(format!("{name}.json"))
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Layout::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn list() -> Vec<String> {
    let Some(dir) = layouts_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| path.file_stem().and_then(|s| s.to_str()).map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// The layout named by `IW4L_LAYOUT`, if set.
pub fn env_layout() -> Option<Result<Layout, String>> {
    let name = std::env::var("IW4L_LAYOUT").ok()?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some(load(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(brush: &Brush, p: [f32; 3]) -> bool {
        brush
            .planes
            .iter()
            .all(|pl| dot([pl[0], pl[1], pl[2]], p) <= pl[3] + 1e-3)
    }

    fn layout(shapes: &str) -> Layout {
        Layout::parse(&format!(
            r#"{{ "name": "t", "base_map": "mp_highrise", "shapes": [{shapes}] }}"#
        ))
        .unwrap()
    }

    #[test]
    fn box_brush_contains_its_volume() {
        let l = layout(
            r#"{ "type": "box", "center": [10, 20, 30], "size": [100, 40, 8], "angles": [0, 30, 0] }"#,
        );
        let brush = &l.brushes()[0];
        assert_eq!(brush.planes.len(), 6);
        assert!(inside(brush, [10.0, 20.0, 30.0]));
        assert!(!inside(brush, [10.0, 20.0, 40.0]));
        let along = rotation([0.0, 30.0, 0.0])[0];
        assert!(inside(brush, add([10.0, 20.0, 30.0], scale(along, 49.0))));
        assert!(!inside(brush, add([10.0, 20.0, 30.0], scale(along, 51.0))));
    }

    #[test]
    fn ramp_faces_are_too_steep_to_stand_on() {
        let l = layout(r#"{ "type": "ramp", "center": [0, 0, 0], "length": 512, "width": 256 }"#);
        let brush = &l.brushes()[0];
        assert_eq!(brush.planes.len(), 5);
        let faces: Vec<_> = brush.planes.iter().filter(|p| p[2] > 0.0).collect();
        assert_eq!(faces.len(), 2);
        for face in faces {
            assert!(face[2] < 0.7, "face normal z {} is walkable", face[2]);
        }
        let height = 128.0 * 60f32.to_radians().tan();
        assert!(inside(brush, [0.0, 0.0, height - 1.0]));
        assert!(!inside(brush, [0.0, 0.0, height + 1.0]));
        assert!(!inside(brush, [300.0, 0.0, 10.0]));
    }

    #[test]
    fn ramp_visual_slabs_lie_on_the_faces() {
        let l = layout(
            r#"{ "type": "ramp", "center": [0, 0, 0], "length": 512, "width": 256, "yaw": 90 }"#,
        );
        let visuals = l.visuals();
        assert_eq!(visuals.len(), 2);
        for v in &visuals {
            assert_eq!(v.model, DEFAULT_MODEL);
            let top = add(v.center, scale(v.axes[2], RAMP_VISUAL_THICKNESS * 0.5));
            let brush = &l.brushes()[0];
            assert!(inside(brush, top), "slab top should sit on the ramp face");
        }
    }

    #[test]
    fn invisible_shapes_have_no_visuals() {
        let l = layout(
            r#"{ "type": "box", "center": [0, 0, 0], "size": [1, 1, 1], "visible": false }"#,
        );
        assert!(l.visuals().is_empty());
        assert_eq!(l.brushes().len(), 1);
    }

    #[test]
    fn rejects_bad_shapes_and_fields() {
        let bad = [
            r#"{ "name": "t", "base_map": "", "shapes": [] }"#,
            r#"{ "name": "t", "base_map": "m", "shapes": [{ "type": "box", "center": [0,0,0], "size": [0,1,1] }] }"#,
            r#"{ "name": "t", "base_map": "m", "shapes": [{ "type": "ramp", "center": [0,0,0], "length": 1, "width": 1, "angle": 95 }] }"#,
            r#"{ "name": "t", "base_map": "m", "sahpes": [] }"#,
        ];
        for text in bad {
            assert!(Layout::parse(text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn zone_match_ignores_namespace_and_case() {
        let l = layout("");
        assert!(l.applies_to_zone("mp_highrise"));
        assert!(l.applies_to_zone("iw4:MP_Highrise"));
        assert!(!l.applies_to_zone("mp_rust"));
    }

    #[test]
    fn checkpoints_resolve_by_index_or_name() {
        let l = Layout::parse(
            r#"{ "name": "t", "base_map": "m", "checkpoints": [
                { "name": "start", "volume": { "min": [0,0,0], "max": [64,64,64] }, "origin": [32,32,32] },
                { "name": "mid", "volume": { "min": [100,0,0], "max": [164,64,64] }, "origin": [132,32,32], "yaw": 90 }
            ] }"#,
        )
        .unwrap();
        assert_eq!(l.checkpoints.len(), 2);
        assert_eq!(l.checkpoint_index("0"), Some(0));
        assert_eq!(l.checkpoint_index("1"), Some(1));
        assert_eq!(l.checkpoint_index("mid"), Some(1));
        assert_eq!(l.checkpoint_index("MID"), Some(1));
        assert_eq!(l.checkpoint_index("2"), None);
        assert_eq!(l.checkpoint_index("nope"), None);
    }

    #[test]
    fn rejects_bad_checkpoints() {
        let bad = [
            r#"{ "name": "t", "base_map": "m", "checkpoints": [
                { "volume": { "min": [0,0,0], "max": [64,64,64] }, "origin": [100,0,0] } ] }"#,
            r#"{ "name": "t", "base_map": "m", "checkpoints": [
                { "volume": { "min": [64,0,0], "max": [0,64,64] }, "origin": [32,32,32] } ] }"#,
            r#"{ "name": "t", "base_map": "m", "reset": {
                "volumes": [{ "min": [0,0,0], "max": [64,64,64] }, { "min": [1,1,1] }] } }"#,
        ];
        for text in bad {
            assert!(Layout::parse(text).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn restart_volumes_parse_alongside_fall_volumes() {
        let l = Layout::parse(
            r#"{ "name": "t", "base_map": "m", "reset": {
                "below_z": 10,
                "volumes": [{ "min": [0,0,0], "max": [1,1,1] }],
                "restart_volumes": [{ "min": [2,2,2], "max": [3,3,3] }]
            } }"#,
        )
        .unwrap();
        assert_eq!(l.reset.volumes.len(), 1);
        assert_eq!(l.reset.restart_volumes.len(), 1);
        assert_eq!(l.reset.below_z, Some(10.0));
    }

    #[test]
    fn dropped_ramp_descends_along_its_length() {
        let l = layout(
            r#"{ "type": "ramp", "center": [0, 0, 1000], "length": 1000, "width": 256, "drop": 200 }"#,
        );
        let brush = &l.brushes()[0];
        let height = 128.0 * 60f32.to_radians().tan();
        // The ridge sits `drop / 2` higher at the near end and lower at the far one.
        assert!(inside(brush, [-450.0, 0.0, 1000.0 + height + 90.0 - 5.0]));
        assert!(!inside(brush, [450.0, 0.0, 1000.0 + height - 90.0 + 5.0]));
        assert!(inside(brush, [450.0, 0.0, 1000.0 + height - 90.0 - 5.0]));
        for face in brush.planes.iter().filter(|p| p[2] > 0.0) {
            assert!(
                face[2] < 0.7,
                "tilted face normal z {} is walkable",
                face[2]
            );
        }
        assert!(Layout::parse(
            r#"{ "name": "t", "base_map": "m", "shapes": [{ "type": "ramp", "center": [0,0,0], "length": 100, "width": 10, "drop": 100 }] }"#
        )
        .is_err());
    }
}
