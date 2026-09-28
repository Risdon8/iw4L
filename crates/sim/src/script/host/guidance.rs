use super::args::{arg, float, int, optional, vector};
use super::entities::EntityKind;
use crate::frame::FrameWorld;
use crate::script::Namespace::{Function, Method};
use crate::script::{NativeRegistry, Runtime, Value};
use bevy_ecs::prelude::World;
use glam::Vec3;

const TOP_ATTACK_HEIGHT: f32 = 2000.0;
const TOP_ATTACK_DIVE_RANGE: f32 = 1500.0;
const TURN_RATE_DEG_PER_S: f32 = 240.0;
pub(crate) const ATTRACTOR_SLOTS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Anchor {
    Entity(u64),
    Point([f32; 3]),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Attractor {
    pub anchor: Anchor,
    attracts: bool,
    strength: f32,
    max_dist: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Aim {
    Entity { object: u64, offset: [f32; 3] },
    Point([f32; 3]),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Guide {
    aim: Option<Aim>,
    top: bool,
    diving: bool,
}

fn missile(world: &World, receiver: &Value) -> Result<u64, String> {
    match world.resource::<Runtime>().entity(receiver) {
        Some((object, e)) if matches!(e.kind, EntityKind::Missile(_)) => Ok(object),
        _ => Err("receiver is not a missile".into()),
    }
}

fn guide(
    world: &mut World,
    receiver: &Value,
    edit: impl FnOnce(&mut Guide),
) -> Result<Value, String> {
    let object = missile(world, receiver)?;
    let mut runtime = world.resource_mut::<Runtime>();
    let guide = runtime.engine.guides.entry(object).or_insert(Guide {
        aim: None,
        top: false,
        diving: false,
    });
    edit(guide);
    Ok(Value::Undefined)
}

fn create_attractor(
    world: &mut World,
    attracts: bool,
    anchor: Value,
    args: &[Value],
) -> Result<Value, String> {
    let anchor = match anchor {
        Value::Vector(point) => Anchor::Point(point),
        other => Anchor::Entity(super::natives::engine::entity_id(world, &other)?),
    };
    let strength = float(args, 1)?;
    let max_dist = float(args, 2)?;
    if max_dist <= 0.0 {
        return Err("parameter 3: maxDist must be greater than zero".into());
    }
    let mut runtime = world.resource_mut::<Runtime>();
    let slots = &mut runtime.engine.attractors;
    let index = slots.iter().position(Option::is_none).ok_or_else(|| {
        format!("Ran out of attractor/repulsors.  Max allowed: {ATTRACTOR_SLOTS}")
    })?;
    slots[index] = Some(Attractor {
        anchor,
        attracts,
        strength,
        max_dist,
    });
    Ok(Value::Int(index as i32))
}

pub(crate) fn register(registry: &mut NativeRegistry) {
    registry.register(Function, "missile_createattractorent", |world, _, args| {
        create_attractor(world, true, arg(args, 0)?.clone(), args)
    });
    registry.register(
        Function,
        "missile_createattractororigin",
        |world, _, args| create_attractor(world, true, Value::Vector(vector(args, 0)?), args),
    );
    registry.register(Function, "missile_createrepulsorent", |world, _, args| {
        create_attractor(world, false, arg(args, 0)?.clone(), args)
    });
    registry.register(
        Function,
        "missile_createrepulsororigin",
        |world, _, args| create_attractor(world, false, Value::Vector(vector(args, 0)?), args),
    );
    registry.register(Function, "missile_deleteattractor", |world, _, args| {
        let index = usize::try_from(int(args, 0)?)
            .ok()
            .filter(|&index| index < ATTRACTOR_SLOTS)
            .ok_or("parameter 1: Invalid attractor or repulsor")?;
        world.resource_mut::<Runtime>().engine.attractors[index] = None;
        Ok(Value::Undefined)
    });
    registry.register(Method, "missile_settargetent", |world, receiver, args| {
        let object = super::natives::engine::entity_id(world, arg(args, 0)?)?;
        let offset = optional(args, 1, vector)?.unwrap_or([0.0; 3]);
        guide(world, receiver, |g| {
            g.aim = Some(Aim::Entity { object, offset })
        })
    });
    registry.register(Method, "missile_settargetpos", |world, receiver, args| {
        let point = vector(args, 0)?;
        guide(world, receiver, |g| g.aim = Some(Aim::Point(point)))
    });
    registry.register(Method, "missile_cleartarget", |world, receiver, _| {
        guide(world, receiver, |g| g.aim = None)
    });
    registry.register(
        Method,
        "missile_setflightmodedirect",
        |world, receiver, _| {
            guide(world, receiver, |g| {
                g.top = false;
                g.diving = false;
            })
        },
    );
    registry.register(Method, "missile_setflightmodetop", |world, receiver, _| {
        guide(world, receiver, |g| {
            g.top = true;
            g.diving = false;
        })
    });
}

fn aim_point(world: &mut World, aim: Aim) -> Option<[f32; 3]> {
    match aim {
        Aim::Point(point) => Some(point),
        Aim::Entity { object, offset } => {
            if !world.resource::<Runtime>().live(&object) {
                return None;
            }
            match super::players::entity_field(world, object, "origin") {
                Value::Vector(o) => Some([o[0] + offset[0], o[1] + offset[1], o[2] + offset[2]]),
                _ => None,
            }
        }
    }
}

pub(crate) fn turn_toward(current: Vec3, wanted: Vec3, max_radians: f32) -> Vec3 {
    let angle = current.angle_between(wanted);
    if angle <= max_radians || angle.is_nan() {
        return wanted;
    }
    let axis = current.cross(wanted);
    let axis = if axis.length_squared() < 1e-8 {
        current.any_orthonormal_vector()
    } else {
        axis.normalize()
    };
    glam::Quat::from_axis_angle(axis, max_radians) * current
}

fn attract(world: &mut World, now: i32) {
    let slots: Vec<Attractor> = world
        .resource::<Runtime>()
        .engine
        .attractors
        .iter()
        .flatten()
        .copied()
        .collect();
    if slots.is_empty() {
        return;
    }
    let mut anchors = Vec::with_capacity(slots.len());
    for slot in slots {
        let origin = match slot.anchor {
            Anchor::Point(point) => Some(Vec3::from_array(point)),
            Anchor::Entity(object) => match super::players::entity_field(world, object, "origin") {
                Value::Vector(o) => Some(Vec3::from_array(o)),
                _ => None,
            },
        };
        anchors.extend(origin.map(|origin| (slot, origin)));
    }
    let seconds = crate::MATCH_TICK_MS as f32 * 0.001;
    for projectile in crate::frame::collect_projectiles(world) {
        if !projectile.live {
            continue;
        }
        let runtime = world.resource::<Runtime>();
        let steered = runtime
            .missiles
            .get(&projectile.id)
            .and_then(|object| runtime.engine.guides.get(object))
            .is_some_and(|guide| guide.aim.is_some());
        let mut frame = FrameWorld::from_world(world);
        let rocket = frame
            .combat_facts_for(projectile.weapon)
            .is_some_and(|facts| facts.weap_type == super::weapons::WEAPTYPE_PROJECTILE);
        if steered || !rocket {
            continue;
        }
        let origin = Vec3::from_array(projectile.origin_at(now));
        let velocity = Vec3::from_array(projectile.velocity);
        let speed = velocity.length();
        let Some(forward) = velocity.try_normalize() else {
            continue;
        };
        let mut force = Vec3::ZERO;
        for (slot, anchor) in &anchors {
            let delta = *anchor - origin;
            let ahead = delta.dot(forward);
            if ahead <= 0.0 {
                continue;
            }
            let perp = delta - forward * ahead;
            let mut side = perp.length();
            let mut dir = if side < 1e-5 {
                if slot.attracts {
                    continue;
                }
                Vec3::NEG_Z
            } else {
                perp / side
            };
            if !slot.attracts && dir.z > 0.0 {
                dir = Vec3::NEG_Z;
                side = 0.0;
            }
            let distance = delta.length();
            if distance > slot.max_dist {
                continue;
            }
            let angle = (side.abs() / ahead).atan() * std::f32::consts::FRAC_2_PI;
            let mut push = (1.0 - distance / slot.max_dist)
                * slot.strength
                * if slot.attracts { angle } else { angle - 1.0 };
            if slot.attracts {
                push = push.min(speed * side / ahead / seconds);
            }
            force += dir * push;
        }
        if force == Vec3::ZERO {
            continue;
        }
        let Some(dir) = (velocity + force * seconds).try_normalize() else {
            continue;
        };
        let delta = entity_iw4::truncated_tr_delta((dir * speed).to_array());
        if let Some(projectile) = frame.projectile_mut_by_number(projectile.entnum) {
            projectile.velocity = delta;
            projectile.pos = entity_iw4::Trajectory {
                tr_time: now,
                tr_type: entity_iw4::TR_LINEAR,
                tr_duration: 0,
                tr_delta: delta,
                tr_base: origin.to_array(),
            };
            projectile.apos = entity_iw4::fire_missile_apos(dir.to_array());
        }
    }
}

pub(crate) fn advance(world: &mut World) {
    let now = crate::level_time_ms(world.resource::<crate::step::StepRequest>().tick);
    attract(world, now);
    let guides: Vec<(u64, Guide)> = world
        .resource::<Runtime>()
        .engine
        .guides
        .iter()
        .map(|(object, guide)| (*object, *guide))
        .collect();
    for (object, mut guide) in guides {
        let found = world
            .resource::<Runtime>()
            .entities
            .get(&object)
            .and_then(|e| match e.kind {
                EntityKind::Missile(id) => Some((id, e.number)),
                _ => None,
            });
        let Some((id, number)) = found else {
            world
                .resource_mut::<Runtime>()
                .engine
                .guides
                .remove(&object);
            continue;
        };
        let live = FrameWorld::from_world(world)
            .projectile_by_number(number)
            .filter(|p| p.id == id && p.live);
        let Some(projectile) = live else {
            world
                .resource_mut::<Runtime>()
                .engine
                .guides
                .remove(&object);
            continue;
        };
        let Some(target) = guide.aim.and_then(|aim| aim_point(world, aim)) else {
            continue;
        };
        let origin = Vec3::from_array(projectile.origin_at(now));
        let velocity = Vec3::from_array(projectile.velocity);
        let speed = velocity.length();
        if speed < 1.0 {
            continue;
        }
        let target = Vec3::from_array(target);
        let flat = (target - origin).truncate().length();
        let mut goal = target;
        if guide.top && !guide.diving {
            if flat <= TOP_ATTACK_DIVE_RANGE || origin.z >= target.z + TOP_ATTACK_HEIGHT {
                guide.diving = true;
            } else {
                goal = Vec3::new(target.x, target.y, target.z + TOP_ATTACK_HEIGHT);
            }
        }
        let Some(wanted) = (goal - origin).try_normalize() else {
            continue;
        };
        let seconds = crate::MATCH_TICK_MS as f32 * 0.001;
        let dir = turn_toward(
            velocity / speed,
            wanted,
            TURN_RATE_DEG_PER_S.to_radians() * seconds,
        );
        let delta = entity_iw4::truncated_tr_delta((dir * speed).to_array());
        let mut frame = FrameWorld::from_world(world);
        if let Some(projectile) = frame.projectile_mut_by_number(number) {
            projectile.velocity = delta;
            projectile.pos = entity_iw4::Trajectory {
                tr_time: now,
                tr_type: entity_iw4::TR_LINEAR,
                tr_duration: 0,
                tr_delta: delta,
                tr_base: origin.to_array(),
            };
            projectile.apos = entity_iw4::fire_missile_apos(dir.to_array());
        }
        world
            .resource_mut::<Runtime>()
            .engine
            .guides
            .insert(object, guide);
    }
}
