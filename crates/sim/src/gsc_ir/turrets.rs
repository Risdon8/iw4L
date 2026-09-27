use super::entities::EntityKind;
use super::iw4_natives::string;
use super::natives_engine::{TraceIgnore, collider_entity, entity_id, entity_trace, keyed_array};
use super::natives_math::{arg, float, int, optional, vector};
use super::runtime::raise;
use super::*;
use crate::bullet_collision::{ColliderId, MASK_SHOT, TraceOutcome};
use crate::frame::FrameWorld;
use crate::world::ClientId;
use bevy_ecs::prelude::World;
use glam::Vec3;
use natives::Namespace::{Function, Method};

const TURN_RATE_DEG_PER_S: f32 = 360.0;
const ON_TARGET_DEGREES: f32 = 2.0;
const DEFAULT_RANGE: f32 = 4096.0;
const MUZZLE_HEIGHT: f32 = 40.0;
const TARGET_HEIGHT: f32 = 40.0;
const PLACE_DISTANCE: f32 = 40.0;
const PLACE_DROP: f32 = 64.0;
const PLACE_MAX_SLOPE_COS: f32 = 0.7;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Turret {
    weapon: u32,
    mode: Arc<str>,
    owner: Option<u64>,
    team: Option<Arc<str>>,
    carried: bool,
    operable: bool,
    fire_enabled: bool,
    manual: Option<(u64, [f32; 3])>,
    target: Option<u64>,
    firing: bool,
    on_target: bool,
    aim: [f32; 3],
    arcs: [f32; 4],
    drop_pitch: f32,
    convergence: [f32; 2],
    minimap: bool,
    solid: bool,
    minigun: bool,
    mode_change_wait: f32,
}

fn turret_of(world: &World, receiver: &Value) -> Result<u64, String> {
    let runtime = world.resource::<Runtime>();
    match runtime.entity(receiver) {
        Some((object, _)) if runtime.engine.turrets.contains_key(&object) => Ok(object),
        _ => Err("receiver is not a turret".into()),
    }
}

fn edit(
    world: &mut World,
    receiver: &Value,
    change: impl FnOnce(&mut Turret),
) -> Result<Value, String> {
    let object = turret_of(world, receiver)?;
    change(
        world
            .resource_mut::<Runtime>()
            .engine
            .turrets
            .get_mut(&object)
            .unwrap(),
    );
    Ok(Value::Undefined)
}

fn player_object(world: &World, value: &Value) -> Result<Option<u64>, String> {
    match value {
        Value::Undefined => Ok(None),
        Value::Object(id) if world.resource::<Runtime>().player_client(*id).is_some() => {
            Ok(Some(*id))
        }
        _ => Err("owner is not a player".into()),
    }
}

fn field_vector(world: &mut World, object: u64, name: &str) -> [f32; 3] {
    match super::players::entity_field(world, object, name) {
        Value::Vector(v) => v,
        _ => [0.0; 3],
    }
}

fn muzzle(world: &mut World, object: u64) -> [f32; 3] {
    if let Some((origin, _)) = super::presence::tag_world(world, object, "tag_flash") {
        return origin;
    }
    let origin = field_vector(world, object, "origin");
    [origin[0], origin[1], origin[2] + MUZZLE_HEIGHT]
}

fn ignore_self(world: &World, object: u64) -> TraceIgnore {
    TraceIgnore {
        model: world
            .resource::<Runtime>()
            .entities
            .get(&object)
            .and_then(|e| e.presence),
        ..Default::default()
    }
}

fn weapon_range(world: &mut World, weapon: u32) -> f32 {
    FrameWorld::from_world(world)
        .combat_facts_for(weapon)
        .map(|f| f.bullet_range())
        .filter(|r| *r > 0.0)
        .unwrap_or(DEFAULT_RANGE)
}

pub(super) fn register(registry: &mut NativeRegistry) {
    registry.register(Function, "spawnturret", |world, _, args| {
        let classname = string(args, 0)?;
        let origin = vector(args, 1)?;
        let weaponinfo = string(args, 2)?;
        let weapon =
            crate::script_player::weapon_named(&FrameWorld::from_world(world), &weaponinfo)?;
        let presence = super::presence::spawn_presence(world, origin)?;
        let mut runtime = world.resource_mut::<Runtime>();
        let id = runtime.create_entity(EntityKind::Spawned, &classname)?;
        runtime.set_object_field(id, "origin", Value::Vector(origin));
        runtime.set_object_field(id, "angles", Value::Vector([0.0; 3]));
        runtime.set_object_field(id, "weaponinfo", Value::String(weaponinfo.into()));
        runtime.entities.get_mut(&id).unwrap().presence = Some(presence);
        runtime.engine.turrets.insert(
            id,
            Turret {
                weapon,
                mode: "manual".into(),
                owner: None,
                team: None,
                carried: false,
                operable: true,
                fire_enabled: true,
                manual: None,
                target: None,
                firing: false,
                on_target: false,
                aim: [1.0, 0.0, 0.0],
                arcs: [180.0, 180.0, 90.0, 90.0],
                drop_pitch: 0.0,
                convergence: [0.0; 2],
                minimap: false,
                solid: false,
                minigun: false,
                mode_change_wait: 0.0,
            },
        );
        Ok(Value::Object(id))
    });
    registry.register(Function, "canspawnturret", |_, _, _| Ok(Value::Int(1)));

    registry.register(Method, "setmode", |world, receiver, args| {
        let mode: Arc<str> = string(args, 0)?.into();
        if !matches!(
            &*mode,
            "auto_ai" | "manual" | "manual_ai" | "auto_nonai" | "sentry" | "sentry_offline"
        ) {
            return Err(format!("unknown turret mode '{mode}'"));
        }
        edit(world, receiver, |t| t.mode = mode)
    });
    fn set_target(world: &mut World, receiver: &Value, args: &[Value]) -> Result<Value, String> {
        let target = entity_id(world, arg(args, 0)?)?;
        let offset = optional(args, 1, vector)?.unwrap_or([0.0; 3]);
        edit(world, receiver, |t| t.manual = Some((target, offset)))
    }
    registry.register(Method, "settargetentity", set_target);
    registry.register(Method, "setturrettargetent", set_target);
    registry.register(Method, "cleartargetentity", |world, receiver, _| {
        edit(world, receiver, |t| t.manual = None)
    });
    registry.register(Method, "getturrettarget", |world, receiver, _| {
        let object = turret_of(world, receiver)?;
        let runtime = world.resource::<Runtime>();
        Ok(runtime.engine.turrets[&object]
            .target
            .filter(|t| runtime.objects.contains_key(t))
            .map_or(Value::Undefined, Value::Object))
    });
    registry.register(Method, "getturretowner", |world, receiver, _| {
        let object = turret_of(world, receiver)?;
        Ok(world.resource::<Runtime>().engine.turrets[&object]
            .owner
            .map_or(Value::Undefined, Value::Object))
    });
    registry.register(Method, "isfiringturret", |world, receiver, _| {
        let runtime = world.resource::<Runtime>();
        Ok(Value::Int(
            runtime
                .entity(receiver)
                .and_then(|(object, _)| runtime.engine.turrets.get(&object))
                .is_some_and(|t| t.firing)
                .into(),
        ))
    });
    registry.register(Method, "shootturret", |world, receiver, _| {
        let object = turret_of(world, receiver)?;
        shoot(world, object);
        Ok(Value::Undefined)
    });
    registry.register(Method, "setturretteam", |world, receiver, args| {
        let team: Arc<str> = string(args, 0)?.into();
        edit(world, receiver, |t| t.team = Some(team))
    });
    registry.register(Method, "setsentryowner", |world, receiver, args| {
        let owner = player_object(world, arg(args, 0)?)?;
        edit(world, receiver, |t| t.owner = owner)
    });
    registry.register(Method, "setsentrycarried", |world, receiver, args| {
        let carried = player_object(world, arg(args, 0)?)?.is_some();
        edit(world, receiver, |t| t.carried = carried)
    });
    registry.register(Method, "maketurretinoperable", |world, receiver, _| {
        edit(world, receiver, |t| t.operable = false)
    });
    registry.register(Method, "maketurretoperable", |world, receiver, _| {
        edit(world, receiver, |t| t.operable = true)
    });
    registry.register(Method, "turretfiredisable", |world, receiver, _| {
        edit(world, receiver, |t| t.fire_enabled = false)
    });
    registry.register(Method, "turretfireenable", |world, receiver, _| {
        edit(world, receiver, |t| t.fire_enabled = true)
    });
    registry.register(Method, "maketurretsolid", |world, receiver, _| {
        edit(world, receiver, |t| t.solid = true)
    });
    registry.register(Method, "setsentryminigun", |world, receiver, _| {
        edit(world, receiver, |t| t.minigun = true)
    });
    registry.register(
        Method,
        "setturretminimapvisible",
        |world, receiver, args| {
            let visible = int(args, 0)? != 0;
            edit(world, receiver, |t| t.minimap = visible)
        },
    );
    registry.register(
        Method,
        "setturretmodechangewait",
        |world, receiver, args| {
            let wait = optional(args, 0, int)?.map_or(1.0, |n| n as f32);
            edit(world, receiver, |t| t.mode_change_wait = wait)
        },
    );
    registry.register(Method, "setdefaultdroppitch", |world, receiver, args| {
        let pitch = float(args, 0)?;
        edit(world, receiver, |t| t.drop_pitch = pitch)
    });
    registry.register(Method, "setconvergencetime", |world, receiver, args| {
        let seconds = float(args, 0)?;
        let axis = optional(args, 1, string)?;
        edit(world, receiver, |t| match axis.as_deref() {
            Some("pitch") => t.convergence[0] = seconds,
            _ => t.convergence[1] = seconds,
        })
    });
    fn arc(
        world: &mut World,
        receiver: &Value,
        args: &[Value],
        index: usize,
    ) -> Result<Value, String> {
        let degrees = float(args, 0)?.clamp(0.0, 180.0);
        edit(world, receiver, |t| t.arcs[index] = degrees)
    }
    registry.register(Method, "setleftarc", |w, r, a| arc(w, r, a, 0));
    registry.register(Method, "setrightarc", |w, r, a| arc(w, r, a, 1));
    registry.register(Method, "settoparc", |w, r, a| arc(w, r, a, 2));
    registry.register(Method, "setbottomarc", |w, r, a| arc(w, r, a, 3));

    registry.register(Method, "canplayerplacesentry", |world, receiver, _| {
        let client = super::natives_player::player(world, receiver)?;
        let (origin, yaw) = FrameWorld::from_world(world)
            .player(ClientId(client))
            .map_or(([0.0; 3], 0.0), |ps| (ps.origin, ps.viewangles[1]));
        let (sin, cos) = yaw.to_radians().sin_cos();
        let ahead = [
            origin[0] + cos * PLACE_DISTANCE,
            origin[1] + sin * PLACE_DISTANCE,
            origin[2] + 16.0,
        ];
        let ignore = TraceIgnore {
            client: Some(ClientId(client)),
            ..Default::default()
        };
        let mask = MASK_SHOT & !crate::bullet_collision::CONTENTS_BODY;
        let blocked = !matches!(
            entity_trace(world, [origin[0], origin[1], ahead[2]], ahead, mask, ignore),
            TraceOutcome::Miss { .. }
        );
        let below = [ahead[0], ahead[1], ahead[2] - PLACE_DROP];
        let (spot, ground) = match entity_trace(world, ahead, below, mask, ignore) {
            TraceOutcome::Hit { end, normal, .. } => (end, normal[2] >= PLACE_MAX_SLOPE_COS),
            TraceOutcome::Miss { end } => (end, false),
            _ => (ahead, false),
        };
        keyed_array(
            world,
            vec![
                ("origin", Value::Vector(spot)),
                ("angles", Value::Vector([0.0, yaw, 0.0])),
                ("result", Value::Int((ground && !blocked).into())),
            ],
        )
    });
}

fn aim_point(world: &mut World, object: u64, offset: [f32; 3]) -> Option<Vec3> {
    if !world.resource::<Runtime>().objects.contains_key(&object) {
        return None;
    }
    let origin = field_vector(world, object, "origin");
    Some(Vec3::from_array(origin) + Vec3::from_array(offset))
}

fn hostile(world: &mut World, turret: &Turret, target: u64) -> bool {
    if Some(target) == turret.owner {
        return false;
    }
    let team = |world: &mut World, object: u64| match super::players::entity_field(
        world, object, "team",
    ) {
        Value::String(team) => Some(team),
        _ => None,
    };
    let own = turret
        .team
        .clone()
        .or_else(|| turret.owner.and_then(|o| team(world, o)));
    match own.as_deref() {
        None | Some("free") => true,
        Some(own) => team(world, target).as_deref() != Some(own),
    }
}

fn visible(world: &mut World, from: [f32; 3], to: Vec3, ignore: TraceIgnore, target: u64) -> bool {
    match entity_trace(world, from, to.to_array(), MASK_SHOT, ignore) {
        TraceOutcome::Miss { .. } => true,
        TraceOutcome::Hit { collider, .. } => {
            collider_entity(world, collider) == Value::Object(target)
        }
        _ => false,
    }
}

fn within_arcs(turret: &Turret, base: [f32; 3], dir: Vec3) -> bool {
    let angles = math_iw4::vect_to_angles(dir.to_array());
    let yaw = math_iw4::angle_subtract(angles[1], base[1]);
    let pitch = math_iw4::angle_subtract(angles[0], base[0]);
    let [left, right, top, bottom] = turret.arcs;
    (-right..=left).contains(&yaw) && (-top..=bottom).contains(&pitch)
}

fn acquire(world: &mut World, object: u64, turret: &Turret, from: [f32; 3]) -> Option<(u64, Vec3)> {
    if let Some((target, offset)) = turret.manual {
        return aim_point(world, target, offset).map(|at| (target, at));
    }
    if !matches!(&*turret.mode, "sentry" | "auto_ai" | "auto_nonai") {
        return None;
    }
    let base = field_vector(world, object, "angles");
    let range = weapon_range(world, turret.weapon);
    let ignore = ignore_self(world, object);
    let candidates: Vec<(u32, u64)> = world
        .resource::<Runtime>()
        .players
        .iter()
        .filter(|(_, slot)| &*slot.sessionstate == "playing")
        .map(|(client, slot)| (*client, slot.object))
        .collect();
    let mut best: Option<(f32, u64, Vec3)> = None;
    for (client, player) in candidates {
        let alive = {
            let frame = FrameWorld::from_world(world);
            frame
                .player(ClientId(client))
                .and_then(|ps| (ps.health > 0).then_some(ps.origin))
        };
        let Some(origin) = alive else { continue };
        if !hostile(world, turret, player) {
            continue;
        }
        let at = Vec3::from_array(origin) + Vec3::Z * TARGET_HEIGHT;
        let delta = at - Vec3::from_array(from);
        let distance = delta.length();
        if distance > range || best.is_some_and(|(d, ..)| d <= distance) {
            continue;
        }
        let Some(dir) = delta.try_normalize() else {
            continue;
        };
        if !within_arcs(turret, base, dir) || !visible(world, from, at, ignore, player) {
            continue;
        }
        best = Some((distance, player, at));
    }
    best.map(|(_, player, at)| (player, at))
}

pub(super) fn advance(world: &mut World) {
    let objects: Vec<u64> = world
        .resource::<Runtime>()
        .engine
        .turrets
        .keys()
        .copied()
        .collect();
    for object in objects {
        if !world.resource::<Runtime>().entities.contains_key(&object) {
            world
                .resource_mut::<Runtime>()
                .engine
                .turrets
                .remove(&object);
            continue;
        }
        let mut turret = world.resource::<Runtime>().engine.turrets[&object].clone();
        let from = muzzle(world, object);
        let active = turret.operable && !turret.carried && &*turret.mode != "sentry_offline";
        let found = if active {
            acquire(world, object, &turret, from)
        } else {
            None
        };
        let was_firing = turret.firing;
        let was_on_target = turret.on_target;
        match found {
            Some((target, at)) => {
                if turret.target != Some(target) {
                    turret.on_target = false;
                }
                turret.target = Some(target);
                if let Some(wanted) = (at - Vec3::from_array(from)).try_normalize() {
                    let step =
                        TURN_RATE_DEG_PER_S.to_radians() * crate::MATCH_TICK_MS as f32 * 0.001;
                    let aim =
                        super::guidance::turn_toward(Vec3::from_array(turret.aim), wanted, step);
                    turret.aim = aim.to_array();
                    turret.on_target = aim.angle_between(wanted) <= ON_TARGET_DEGREES.to_radians();
                }
            }
            None => {
                turret.target = None;
                turret.on_target = false;
            }
        }
        turret.firing = turret.on_target && turret.fire_enabled;
        let raise_on_target = turret.on_target && !was_on_target;
        let raise_state = turret.firing != was_firing;
        world
            .resource_mut::<Runtime>()
            .engine
            .turrets
            .insert(object, turret);
        if raise_on_target {
            raise(world, Value::Object(object), "turret_on_target", Vec::new());
        }
        if raise_state {
            raise(
                world,
                Value::Object(object),
                "turretstatechange",
                Vec::new(),
            );
        }
    }
}

fn shoot(world: &mut World, object: u64) {
    let turret = world.resource::<Runtime>().engine.turrets[&object].clone();
    let from = muzzle(world, object);
    let range = weapon_range(world, turret.weapon);
    let dir = Vec3::from_array(turret.aim);
    let end = (Vec3::from_array(from) + dir * range).to_array();
    let ignore = ignore_self(world, object);
    let TraceOutcome::Hit { collider, .. } = entity_trace(world, from, end, MASK_SHOT, ignore)
    else {
        return;
    };
    let amount = FrameWorld::from_world(world)
        .combat_facts_for(turret.weapon)
        .map_or(0, |f| f.damage);
    let hit = match collider {
        ColliderId::Player { .. } | ColliderId::World { .. } => None,
        _ => match collider_entity(world, collider) {
            Value::Object(hit) => Some(hit),
            _ => return,
        },
    };
    let runtime = world.resource::<Runtime>();
    let target = match collider {
        ColliderId::Player { client, .. } => super::HitTarget::Player(client),
        ColliderId::World { .. } => return,
        _ => match hit.and_then(|hit| runtime.entities.get(&hit)) {
            Some(e) if e.can_damage => match e.presence {
                Some(presence) => super::HitTarget::Entity(presence),
                None => return,
            },
            _ => return,
        },
    };
    let attacker = turret
        .owner
        .and_then(|o| runtime.player_client(o))
        .map(ClientId);
    let inflictor = runtime.entities.get(&object).and_then(|e| e.presence);
    world.resource_mut::<Runtime>().hits.push(super::ScriptHit {
        target,
        amount,
        origin: from,
        attacker,
        inflictor,
        means: "MOD_RIFLE_BULLET",
        weapon: turret.weapon,
        flags: 0,
        hitloc: 0,
    });
}
