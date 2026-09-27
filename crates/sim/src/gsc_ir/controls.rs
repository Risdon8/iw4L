use super::iw4_natives::string;
use super::natives_math::{float, int, optional, vector};
use super::natives_player::{player, send_menu_command, text};
use super::*;
use crate::frame::FrameWorld;
use crate::world::ClientId;
use bevy_ecs::prelude::World;
use natives::Namespace::{Function, Method};
use playerstate_iw4::{PlayerState, buttons, weap_flags};

mod pof {
    pub const THERMAL_VISION: u32 = 0x8;
    pub const THERMAL_VISION_OVERLAY_FOF: u32 = 0x10;
    pub const REMOTE_CAMERA_SOUNDS: u32 = 0x20;
    pub const ALT_SCENE_REAR_VIEW: u32 = 0x40;
    pub const EMP_JAMMED: u32 = 0x400;
    pub const AC130: u32 = 0x8000;
}

pub(crate) const SCRIPT_LOCK: u8 = 0x40;
const LOCKING: u8 = 1;
const LOCKED: u8 = 2;
const TOP: u8 = 4;
const DIRECT: u8 = 8;
const TOO_CLOSE: u8 = 16;
const NO_CLEARANCE: u8 = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MiniMap {
    upper_left: [f32; 2],
    north: [f32; 2],
    size: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScriptLock {
    target: u64,
    offset: [f32; 3],
}

fn with_player<T>(
    world: &mut World,
    receiver: &Value,
    edit: impl FnOnce(&mut PlayerState) -> Result<T, String>,
) -> Result<T, String> {
    let client = player(world, receiver)?;
    let mut frame = FrameWorld::from_world(world);
    let ps = frame
        .player_mut(ClientId(client))
        .ok_or("player has disconnected")?;
    edit(ps)
}

fn other_flag(world: &mut World, receiver: &Value, flag: u32, on: bool) -> Result<Value, String> {
    with_player(world, receiver, |ps| {
        if on {
            ps.other_flags |= flag;
        } else {
            ps.other_flags &= !flag;
        }
        Ok(Value::Undefined)
    })
}

fn truthy(args: &[Value], at: usize) -> Result<bool, String> {
    Ok(optional(args, at, int)?.unwrap_or(1) != 0)
}

fn client_command(
    world: &mut World,
    receiver: &Value,
    name: &str,
    args: &[Value],
) -> Result<Value, String> {
    let client = player(world, receiver)?;
    let args = args
        .iter()
        .map(|arg| match arg {
            Value::Vector(v) => Ok(format!("{} {} {}", v[0], v[1], v[2])),
            other => text(other).map(|t| t.to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    send_menu_command(
        world,
        client,
        crate::MenuCommandKind::Client {
            name: name.to_owned(),
            args,
        },
    );
    Ok(Value::Undefined)
}

fn edit_lock(
    world: &mut World,
    receiver: &Value,
    edit: impl FnOnce(&mut crate::WeaponLock, &PlayerState, u32),
) -> Result<u32, String> {
    let client = player(world, receiver)?;
    let mut frame = FrameWorld::from_world(world);
    let id = ClientId(client);
    let ps = *frame.player(id).ok_or("player has disconnected")?;
    let meta = frame.client_meta_mut(id);
    let life = meta.life_sequence.0;
    edit(&mut meta.weapon_lock, &ps, life);
    Ok(client)
}

fn lock_target(world: &World, args: &[Value]) -> Result<u64, String> {
    super::natives_engine::entity_id(world, super::natives_math::arg(args, 0)?)
}

pub(super) fn register(registry: &mut NativeRegistry) {
    registry.register(Method, "setspreadoverride", |world, receiver, args| {
        let spread = int(args, 0)?;
        if !(1..64).contains(&spread) {
            return Err(format!(
                "setspreadoverride: spread must be between 1 and 63, not {spread}"
            ));
        }
        with_player(world, receiver, |ps| {
            ps.spread_override = spread;
            ps.spread_override_state = 2;
            Ok(Value::Undefined)
        })
    });
    registry.register(Method, "resetspreadoverride", |world, receiver, _| {
        with_player(world, receiver, |ps| {
            ps.spread_override_state = 1;
            ps.aim_spread_scale = 255.0;
            Ok(Value::Undefined)
        })
    });
    registry.register(Method, "player_recoilscaleon", |world, receiver, args| {
        let scale = int(args, 0)?.clamp(0, 100);
        with_player(world, receiver, |ps| {
            ps.recoil_scale = scale;
            ps.weap_flags |= weap_flags::RECOIL_SCALE;
            Ok(Value::Undefined)
        })
    });
    registry.register(Method, "player_recoilscaleoff", |world, receiver, _| {
        with_player(world, receiver, |ps| {
            ps.weap_flags &= !weap_flags::RECOIL_SCALE;
            Ok(Value::Undefined)
        })
    });
    registry.register(Method, "viewkick", |world, receiver, args| {
        let force = int(args, 0)?;
        if force < 0 {
            return Err(format!("viewkick: damage {force} < 0"));
        }
        let origin = vector(args, 1)?;
        with_player(world, receiver, |ps| {
            let blood = ((ps.max_health * force.min(127) + 50) / 100).min(127);
            let from = [
                ps.origin[0] - origin[0],
                ps.origin[1] - origin[1],
                ps.origin[2] - origin[2],
            ];
            if from == [0.0; 3] {
                ps.damage_yaw = 255;
                ps.damage_pitch = 255;
            } else {
                let angles = math_iw4::vect_to_angles(from);
                ps.damage_pitch = (angles[0] / 360.0 * 256.0) as u32 & 0xff;
                ps.damage_yaw = (angles[1] / 360.0 * 256.0) as u32 & 0xff;
            }
            ps.damage_count = blood;
            ps.damage_event = ps.damage_event.wrapping_add(1);
            Ok(Value::Undefined)
        })
    });

    registry.register(Method, "thermalvisionon", |world, receiver, _| {
        other_flag(world, receiver, pof::THERMAL_VISION, true)
    });
    registry.register(Method, "thermalvisionoff", |world, receiver, _| {
        other_flag(world, receiver, pof::THERMAL_VISION, false)
    });
    registry.register(Method, "thermalvisionfofoverlayon", |world, receiver, _| {
        other_flag(world, receiver, pof::THERMAL_VISION_OVERLAY_FOF, true)
    });
    registry.register(
        Method,
        "thermalvisionfofoverlayoff",
        |world, receiver, _| other_flag(world, receiver, pof::THERMAL_VISION_OVERLAY_FOF, false),
    );
    registry.register(Method, "remotecamerasoundscapeon", |world, receiver, _| {
        other_flag(world, receiver, pof::REMOTE_CAMERA_SOUNDS, true)
    });
    registry.register(Method, "remotecamerasoundscapeoff", |world, receiver, _| {
        other_flag(world, receiver, pof::REMOTE_CAMERA_SOUNDS, false)
    });
    registry.register(Method, "setempjammed", |world, receiver, args| {
        let on = truthy(args, 0)?;
        other_flag(world, receiver, pof::EMP_JAMMED, on)
    });
    registry.register(
        Method,
        "setrearviewrenderenabled",
        |world, receiver, args| {
            let on = truthy(args, 0)?;
            other_flag(world, receiver, pof::ALT_SCENE_REAR_VIEW, on)
        },
    );
    registry.register(Method, "startac130", |world, receiver, _| {
        other_flag(world, receiver, pof::AC130, true)
    });
    registry.register(Method, "stopac130", |world, receiver, _| {
        other_flag(world, receiver, pof::AC130, false)
    });
    registry.register(Method, "stunplayer", |world, receiver, args| {
        let client = player(world, receiver)?;
        let on = truthy(args, 0)?;
        let mut frame = FrameWorld::from_world(world);
        if frame.client_meta(ClientId(client)).is_some() {
            frame.client_meta_mut(ClientId(client)).controls.stunned = on;
        }
        Ok(Value::Undefined)
    });

    macro_rules! client_commands {
        ($($name:literal),* $(,)?) => {$(
            registry.register(Method, $name, |world, receiver, args| {
                client_command(world, receiver, $name, args)
            });
        )*};
    }
    client_commands!(
        "stoplocalsound",
        "setblurforplayer",
        "setdepthoffield",
        "visionsetnakedforplayer",
        "visionsetthermalforplayer",
        "visionsetmissilecamforplayer",
        "stoprumble",
    );

    registry.register(Method, "weaponlockstart", |world, receiver, args| {
        let target = lock_target(world, args)?;
        let now = now_ms(world);
        let client = edit_lock(world, receiver, |lock, ps, life| {
            *lock = crate::WeaponLock {
                weapon: ps.weapon,
                life,
                flags: SCRIPT_LOCK | LOCKING,
                sampled_at: now,
                out_of_ads_at: now,
                acquire_started_at: now,
                ..Default::default()
            };
        })?;
        world.resource_mut::<Runtime>().engine.weapon_locks.insert(
            client,
            ScriptLock {
                target,
                offset: [0.0; 3],
            },
        );
        sync_script_locks(world);
        Ok(Value::Undefined)
    });
    registry.register(Method, "weaponlockfinalize", |world, receiver, args| {
        let target = lock_target(world, args)?;
        let offset = optional(args, 1, vector)?.unwrap_or([0.0; 3]);
        let top = optional(args, 2, int)?.unwrap_or(0) != 0;
        let client = edit_lock(world, receiver, |lock, ps, life| {
            if lock.flags & SCRIPT_LOCK == 0 || lock.life != life {
                lock.weapon = ps.weapon;
                lock.life = life;
            }
            lock.flags &= !(TOP | DIRECT);
            lock.flags |= SCRIPT_LOCK | LOCKING | LOCKED | if top { TOP } else { DIRECT };
        })?;
        world
            .resource_mut::<Runtime>()
            .engine
            .weapon_locks
            .insert(client, ScriptLock { target, offset });
        sync_script_locks(world);
        Ok(Value::Undefined)
    });
    registry.register(Method, "weaponlockfree", |world, receiver, _| {
        let client = edit_lock(world, receiver, |lock, _, _| {
            lock.flags &= !(SCRIPT_LOCK | LOCKING | LOCKED | TOP | DIRECT);
        })?;
        world
            .resource_mut::<Runtime>()
            .engine
            .weapon_locks
            .remove(&client);
        Ok(Value::Undefined)
    });
    fn lock_bit(
        world: &mut World,
        receiver: &Value,
        args: &[Value],
        bit: u8,
    ) -> Result<Value, String> {
        let on = int(args, 0)? != 0;
        edit_lock(world, receiver, |lock, _, _| {
            if on {
                lock.flags |= bit;
            } else {
                lock.flags &= !bit;
            }
        })?;
        Ok(Value::Undefined)
    }
    registry.register(
        Method,
        "weaponlocktargettooclose",
        |world, receiver, args| lock_bit(world, receiver, args, TOO_CLOSE),
    );
    registry.register(Method, "weaponlocknoclearance", |world, receiver, args| {
        lock_bit(world, receiver, args, NO_CLEARANCE)
    });

    registry.register(Function, "setminimap", |world, _, args| {
        string(args, 0)?;
        let upper_left = [float(args, 1)?, float(args, 2)?];
        let lower_right = [float(args, 3)?, float(args, 4)?];
        let runtime = world.resource::<Runtime>();
        let north_yaw = runtime
            .engine
            .worldspawn
            .get("northyaw")
            .and_then(|yaw| yaw.trim().parse::<f32>().ok())
            .unwrap_or(0.0)
            .to_radians();
        let north = [north_yaw.cos(), north_yaw.sin()];
        let dx = lower_right[0] - upper_left[0];
        let dy = lower_right[1] - upper_left[1];
        let size = [
            dx * north[1] - dy * north[0],
            -dx * north[0] - dy * north[1],
        ];
        if size[0] < 0.0 || size[1] < 0.0 {
            return Err("setminimap: the corners are the wrong way round".into());
        }
        world.resource_mut::<Runtime>().engine.minimap = Some(MiniMap {
            upper_left,
            north,
            size,
        });
        Ok(Value::Undefined)
    });
    registry.register(Method, "beginlocationselection", |world, receiver, args| {
        let client = player(world, receiver)?;
        let material = string(args, 0)?;
        let choose_direction = optional(args, 1, int)?.unwrap_or(0) != 0;
        let radius = optional(args, 2, float)?.unwrap_or(0.0);
        let mut frame = FrameWorld::from_world(world);
        if frame.client_meta(ClientId(client)).is_some() {
            frame.client_meta_mut(ClientId(client)).location_selection =
                Some(crate::LocationSelection {
                    material,
                    choose_direction,
                    radius,
                });
        }
        Ok(Value::Undefined)
    });
    registry.register(Method, "endlocationselection", |world, receiver, _| {
        let client = player(world, receiver)?;
        let mut frame = FrameWorld::from_world(world);
        if frame.client_meta(ClientId(client)).is_some() {
            frame.client_meta_mut(ClientId(client)).location_selection = None;
        }
        Ok(Value::Undefined)
    });
}

fn now_ms(world: &World) -> i32 {
    crate::level_time_ms(world.resource::<crate::step::StepRequest>().tick)
}

pub(super) fn sync_script_locks(world: &mut World) {
    let locks: Vec<(u32, ScriptLock)> = world
        .resource::<Runtime>()
        .engine
        .weapon_locks
        .iter()
        .map(|(client, lock)| (*client, *lock))
        .collect();
    for (client, lock) in locks {
        let origin = match super::players::entity_field(world, lock.target, "origin") {
            Value::Vector(origin) => origin,
            _ => continue,
        };
        let mut frame = FrameWorld::from_world(world);
        let id = ClientId(client);
        if frame.client_meta(id).is_none() {
            frame
                .ecs()
                .resource_mut::<Runtime>()
                .engine
                .weapon_locks
                .remove(&client);
            continue;
        }
        let meta = frame.client_meta_mut(id);
        if meta.weapon_lock.flags & SCRIPT_LOCK == 0 {
            continue;
        }
        meta.weapon_lock.target = [
            origin[0] + lock.offset[0],
            origin[1] + lock.offset[1],
            origin[2] + lock.offset[2],
        ];
    }
}

pub(crate) fn select_location(
    world: &mut World,
    client: u32,
    cmd: &mut playerstate_iw4::UserCmd,
    old_buttons: u32,
) {
    let selecting = FrameWorld::from_world(world)
        .client_meta(ClientId(client))
        .is_some_and(|m| m.location_selection.is_some());
    if !selecting {
        return;
    }
    let pressed = cmd.buttons & !old_buttons;
    let authority = world
        .resource::<crate::step::StepRequest>()
        .reason
        .advances_authority_world();
    let receiver = world
        .resource::<Runtime>()
        .players
        .get(&client)
        .map(|slot| Value::Object(slot.object));
    if let (true, Some(receiver)) = (authority, receiver) {
        if pressed & buttons::LOCATION_SELECT != 0 {
            let map = world.resource::<Runtime>().engine.minimap;
            let loc = |byte: u8| (f32::from(byte as i8) + 128.0) / 255.0;
            let location = map.map_or([0.0; 3], |map| {
                let x = loc(cmd.selected_location[0]) * map.size[0];
                let y = loc(cmd.selected_location[1]) * map.size[1];
                [
                    x * map.north[1] + map.upper_left[0] - y * map.north[0],
                    map.upper_left[1] - x * map.north[0] - y * map.north[1],
                    0.0,
                ]
            });
            let yaw = f32::from(cmd.selected_location[2]) * (360.0 / 256.0);
            super::runtime::raise(
                world,
                receiver,
                "confirm_location",
                vec![Value::Vector(location), Value::Float(yaw)],
            );
        } else if pressed & buttons::LOCATION_CANCEL != 0 {
            super::runtime::raise(world, receiver, "cancel_location", Vec::new());
        }
    }
    cmd.buttons &= buttons::CROUCH | buttons::PRONE;
}
