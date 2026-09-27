use super::entities::EntityKind;
use super::natives_math::{arg, optional, vector};
use super::*;
use crate::frame::FrameWorld;
use bevy_ecs::prelude::World;
use glam::Vec3;
use natives::Namespace::Method;

const TOP_ATTACK_HEIGHT: f32 = 2000.0;
const TOP_ATTACK_DIVE_RANGE: f32 = 1500.0;
const TURN_RATE_DEG_PER_S: f32 = 240.0;

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

pub(super) fn register(registry: &mut NativeRegistry) {
    registry.register(Method, "missile_settargetent", |world, receiver, args| {
        let object = super::natives_engine::entity_id(world, arg(args, 0)?)?;
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
            if !world.resource::<Runtime>().objects.contains_key(&object) {
                return None;
            }
            match super::players::entity_field(world, object, "origin") {
                Value::Vector(o) => Some([o[0] + offset[0], o[1] + offset[1], o[2] + offset[2]]),
                _ => None,
            }
        }
    }
}

pub(super) fn turn_toward(current: Vec3, wanted: Vec3, max_radians: f32) -> Vec3 {
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

pub(super) fn advance(world: &mut World) {
    let now = crate::level_time_ms(world.resource::<crate::step::StepRequest>().tick);
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
            projectile.apos = entity_iw4::g_fire_missile_apos(dir.to_array());
        }
        world
            .resource_mut::<Runtime>()
            .engine
            .guides
            .insert(object, guide);
    }
}
