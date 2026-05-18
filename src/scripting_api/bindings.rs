//! `world` pymodule — the surface every embedded RustPython VM exposes to
//! script authors. Pure FFI: each pyfunction looks up the active
//! [`crate::scripting_api::ApiContext`] via [`crate::scripting_api::with_ctx`]
//! and forwards into it.
//!
//! Convention for write verbs: build a [`GameCommand`] and call
//! `ctx.queue_command(...)`. The context impl decides whether to permit the
//! command. Read verbs go through `ctx.snapshot()` — pure POD reads, never
//! touch the live ECS.

use crate::game::commands::{
    GameCommand, ItemDestination, ItemReference, ItemSlotRef, RotationDirection,
};
use crate::player::classes::Class;
use crate::player::components::{AttributeKind, PlayerId};
use crate::player::progression::LEVEL_CAP;
use crate::player::skills::Skill;
use crate::scripting_api::snapshots::{
    attribute_map_to_dict, object_to_dict, skill_ranks_to_dict, space_to_dict, vitals_to_dict,
    PlayerView,
};
use crate::scripting_api::{with_ctx, with_ctx_or};
use crate::world::components::{SpaceId, TilePosition};
use crate::world::floor_definitions::FloorTypeId;

const HELP_TEXT: &str = "\
world API cheat sheet — use help(world.<verb>) for details.

Read:
  world.now()                            world.is_admin()
  world.caller_player_id()               world.player()    -> Player | None
  world.players()    -> list[Player]     world.find_player(id) -> Player | None
  world.spaces()                         world.objects([space_id])
  world.object(id)                       world.object_types()
  world.spell_ids()                      world.floor_tile(space_id, z, x, y)
  world.player_has(type_id, count=1)

Player objects (returned by world.player/players/find_player):
  Read-only properties:
    p.id, p.name, p.class_name, p.level, p.xp, p.xp_for_next
    p.skill_points, p.skills, p.attributes, p.vitals
    p.space_id, p.x, p.y, p.z, p.position, p.facing
  Admin mutations (targeted at p regardless of attached caller):
    p.grant_xp(n)                p.set_level(n)
    p.grant_skill_points(n)      p.set_skill(name, rank)
    p.set_attribute(name, val)   p.set_class(name)
    p.full_heal()                p.set_vitals(health=None, mana=None)
    p.teleport(x, y, z=0, space_id=None)
    p.give(type_id, count=1)     p.take(type_id, count=1)

Write (acts on attached caller):
  world.give(type_id, count=1)           world.take(type_id, count=1)
  world.spawn(type_id, x, y, z=0)        world.despawn(object_id)
  world.teleport(x, y, z=0, space_id=None)
  world.set_vitals(health=None, mana=None)
  world.set_object_state(object_id, state)
  world.rotate(object_id, 'cw'|'ccw')
  world.interact(object_id, verb)        world.open_container(object_id)
  world.set_combat_target(object_id=None)
  world.cast_spell(spell_id, target_object_id)
  world.move_item(from_slot, to_slot)
  world.set_floor(space_id, z, x, y, floor_type=None)

Per-character stash (anyone can read/write):
  world.stash_get(key)                   world.stash_set(key, value)
  world.stash_has(key)                   world.stash_delete(key)

Per-character log (quest scripts only):
  world.log_write(subsection, title, body)

Quest hooks (quest scripts only):
  world.set_var(name, value)             world.get_var(name)
  world.complete_quest(qid)              world.fail_quest(qid)

Admin REPL:
  world.attach_player(player_id)         world.attach_player(None)
  world.reset()
";

/// `world` — verbs for inspecting and mutating the live game world.
///
/// Every embedded RustPython VM in the project (the in-game `~` console,
/// the headless admin REPL, and quest scripts) exposes the same `world`
/// module. Some verbs are admin-only (`teleport`, `set_vitals`) and will
/// raise `RuntimeError("not permitted: ...")` from a quest hook context.
///
/// Use `world.help()` for a categorised cheat sheet, or `help(world.<verb>)`
/// / `world.<verb>.__doc__` for details on a specific verb.
#[rustpython_vm::pymodule]
pub mod world_api {
    use super::*;
    use crate::dialog::variable_storage::YarnValueDump;
    use rustpython_vm::convert::ToPyObject;
    use rustpython_vm::function::FuncArgs;
    use rustpython_vm::{pyclass, PyObjectRef, PyPayload, PyResult, VirtualMachine};

    // --- discovery / help -------------------------------------------------

    /// Print a categorised cheat sheet of every `world.*` verb. For
    /// details on a specific verb use `help(world.<verb>)` or
    /// `print(world.<verb>.__doc__)`. Use `dir(world)` to enumerate
    /// without docs.
    #[pyfunction]
    fn help() {
        with_ctx(|ctx| {
            for line in HELP_TEXT.lines() {
                ctx.log(line.to_owned());
            }
        });
    }

    // --- read API ----------------------------------------------------------

    /// Append `message` to the active context's output channel — the admin
    /// console / REPL output buffer, or the quest engine `info!` log. The
    /// REPL also pipes `print()` and `sys.stdout` writes through here.
    #[pyfunction]
    fn log(message: String) {
        with_ctx(|ctx| ctx.log(message));
    }

    /// Current world time of day as a float (seconds since spawn). Useful
    /// for comparing event timestamps inside scripts.
    #[pyfunction]
    fn now() -> f32 {
        with_ctx_or(0.0, |ctx| ctx.snapshot().world_time)
    }

    /// Player id this context represents, or `None` if no player is
    /// attached. In the admin REPL, set this with `world.attach_player(id)`.
    #[pyfunction]
    fn caller_player_id(vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(vm.ctx.none(), |ctx| match ctx.caller_player_id() {
            Some(id) => id.to_pyobject(vm),
            None => vm.ctx.none(),
        })
    }

    /// Backwards-compat alias for `caller_player_id()` used by older quest
    /// scripts.
    #[pyfunction]
    fn player_id(vm: &VirtualMachine) -> PyObjectRef {
        caller_player_id(vm)
    }

    /// `True` when running inside an admin context (the in-game `~` console
    /// or the headless admin REPL); `False` from quest hooks.
    #[pyfunction]
    fn is_admin() -> bool {
        with_ctx_or(false, |ctx| ctx.is_admin())
    }

    /// All object type ids defined in `assets/overworld_objects/` — the
    /// strings you can pass to `world.spawn(...)`. Returns `list[str]`.
    #[pyfunction]
    fn object_types(vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(vm.ctx.new_list(Vec::new()).into(), |ctx| {
            let items = ctx
                .snapshot()
                .object_types
                .iter()
                .map(|s| s.clone().to_pyobject(vm))
                .collect();
            vm.ctx.new_list(items).into()
        })
    }

    /// All spell ids defined in `assets/spells/` — the strings you can pass
    /// to `world.cast_spell(spell_id, target)`. Returns `list[str]`.
    #[pyfunction]
    fn spell_ids(vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(vm.ctx.new_list(Vec::new()).into(), |ctx| {
            let items = ctx
                .snapshot()
                .spell_ids
                .iter()
                .map(|s| s.clone().to_pyobject(vm))
                .collect();
            vm.ctx.new_list(items).into()
        })
    }

    /// All loaded spaces (overworld, dungeons, ephemeral instances).
    /// Returns `list[dict]` with keys `space_id`, `authored_id`, `width`,
    /// `height`, `fill_floor_type`.
    #[pyfunction]
    fn spaces(vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(vm.ctx.new_list(Vec::new()).into(), |ctx| {
            let items = ctx
                .snapshot()
                .spaces
                .iter()
                .map(|s| space_to_dict(s, vm))
                .collect();
            vm.ctx.new_list(items).into()
        })
    }

    /// The current caller as a `Player` object, or `None` if no player is
    /// attached. In the headless admin REPL use `world.attach_player(id)`
    /// first. Properties (`p.level`, `p.skills`, ...) reflect the snapshot
    /// taken at the start of this REPL input; values refresh on the next
    /// prompt, not after a mutation inside the same expression.
    #[pyfunction]
    fn player(vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(vm.ctx.none(), |ctx| {
            let snapshot = ctx.snapshot();
            let id = match ctx.caller_player_id() {
                Some(id) => id,
                None => return vm.ctx.none(),
            };
            match snapshot.players.iter().find(|p| p.player_id == id) {
                Some(p) => PyPlayer {
                    player_id: p.player_id,
                }
                .into_pyobject(vm),
                None => vm.ctx.none(),
            }
        })
    }

    /// Roster of `Player` objects. In admin contexts: every connected
    /// player. In a quest hook: just the caller (for safety / encapsulation).
    #[pyfunction]
    fn players(vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(vm.ctx.new_list(Vec::new()).into(), |ctx| {
            let snapshot = ctx.snapshot();
            let iter: Box<dyn Iterator<Item = _>> = if ctx.is_admin() {
                Box::new(snapshot.players.iter())
            } else {
                let caller = ctx.caller_player_id();
                Box::new(
                    snapshot
                        .players
                        .iter()
                        .filter(move |p| Some(p.player_id) == caller),
                )
            };
            let items = iter
                .map(|p| {
                    PyPlayer {
                        player_id: p.player_id,
                    }
                    .into_pyobject(vm)
                })
                .collect();
            vm.ctx.new_list(items).into()
        })
    }

    /// `find_player(id)` — look up a `Player` by id. Returns `None` when
    /// no such player is connected. Useful for hitting a specific player
    /// without bothering with `attach_player`:
    /// `world.find_player(7).grant_xp(2000)`.
    #[pyfunction]
    fn find_player(player_id: i64, vm: &VirtualMachine) -> PyObjectRef {
        if player_id < 0 {
            return vm.ctx.none();
        }
        let target = player_id as u64;
        with_ctx_or(vm.ctx.none(), |ctx| {
            match ctx
                .snapshot()
                .players
                .iter()
                .find(|p| p.player_id == target)
            {
                Some(_) => PyPlayer { player_id: target }.into_pyobject(vm),
                None => vm.ctx.none(),
            }
        })
    }

    /// `objects([space_id])` — all world objects, optionally filtered to a
    /// single space. Returns `list[dict]` with `object_id`, `type_id`,
    /// position, vitals, state, and capability flags (`is_npc`,
    /// `is_container`, `is_movable`, `is_rotatable`, `has_dialog`).
    #[pyfunction]
    fn objects(args: FuncArgs, vm: &VirtualMachine) -> PyObjectRef {
        let space_filter: Option<u64> = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n.max(0) as u64);
        with_ctx_or(vm.ctx.new_list(Vec::new()).into(), |ctx| {
            let items = ctx
                .snapshot()
                .objects
                .iter()
                .filter(|o| match space_filter {
                    Some(sid) => o.space_id == sid,
                    None => true,
                })
                .map(|o| object_to_dict(o, vm))
                .collect();
            vm.ctx.new_list(items).into()
        })
    }

    /// Look up a single world object by `object_id`. Returns `None` when
    /// no object with that id exists.
    #[pyfunction]
    fn object(object_id: i64, vm: &VirtualMachine) -> PyObjectRef {
        if object_id < 0 {
            return vm.ctx.none();
        }
        let target = object_id as u64;
        with_ctx_or(vm.ctx.none(), |ctx| {
            match ctx
                .snapshot()
                .objects
                .iter()
                .find(|o| o.object_id == target)
            {
                Some(o) => object_to_dict(o, vm),
                None => vm.ctx.none(),
            }
        })
    }

    /// `floor_tile(space_id, z, x, y)` — floor type id at the given tile,
    /// or `None` if out of bounds / unknown. Use `world.set_floor(...)` to
    /// change a floor.
    #[pyfunction]
    fn floor_tile(args: FuncArgs, vm: &VirtualMachine) -> PyObjectRef {
        let space_id: u64 = match args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
        {
            Some(n) if n >= 0 => n as u64,
            _ => return vm.ctx.none(),
        };
        let z: i32 = args
            .args
            .get(1)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .unwrap_or(0);
        let x: i32 = match args
            .args
            .get(2)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
        {
            Some(n) => n as i32,
            None => return vm.ctx.none(),
        };
        let y: i32 = match args
            .args
            .get(3)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
        {
            Some(n) => n as i32,
            None => return vm.ctx.none(),
        };
        with_ctx_or(vm.ctx.none(), |ctx| {
            let Some(map) = ctx.snapshot().floor_maps.get(&(space_id, z)) else {
                return vm.ctx.none();
            };
            match map.get(x, y) {
                Some(s) => s.to_owned().to_pyobject(vm),
                None => vm.ctx.none(),
            }
        })
    }

    /// `player_has(type_id, count=1)` — `True` when the caller's inventory
    /// holds at least `count` of `type_id` (e.g. `world.player_has("apple",
    /// 3)`).
    #[pyfunction]
    fn player_has(args: FuncArgs, vm: &VirtualMachine) -> bool {
        let type_id: String = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<String>(vm).ok())
            .unwrap_or_default();
        let count: u32 = args
            .args
            .get(1)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n.max(0) as u32)
            .unwrap_or(1);
        with_ctx_or(false, |ctx| ctx.caller_inventory_count(&type_id) >= count)
    }

    // --- inventory + variable verbs (quest-friendly) -----------------------

    /// `give(type_id, count=1)` — add items to the caller's inventory.
    /// Raises `ValueError` if `type_id` is empty.
    #[pyfunction]
    fn give(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        let (type_id, count) = parse_item_count(&args, 1, vm);
        if type_id.is_empty() {
            return Err(vm.new_value_error("give: type_id is required".to_owned()));
        }
        queue(vm, GameCommand::GiveItem { type_id, count })
    }

    /// `take(type_id, count=1)` — remove items from the caller's
    /// inventory. No-op if the caller doesn't have enough of `type_id`.
    #[pyfunction]
    fn take(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        let (type_id, count) = parse_item_count(&args, 1, vm);
        if type_id.is_empty() {
            return Err(vm.new_value_error("take: type_id is required".to_owned()));
        }
        queue(vm, GameCommand::TakeItem { type_id, count })
    }

    /// Quest-script alias for `world.give(type_id, count=1)`.
    #[pyfunction]
    fn player_give(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        give(args, vm)
    }

    /// Quest-script alias for `world.take(type_id, count=1)`.
    #[pyfunction]
    fn player_take(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        take(args, vm)
    }

    /// `set_var(name, value)` — write to a Yarn dialog variable. **Quest
    /// hooks only**; raises `RuntimeError` from admin contexts. `value`
    /// must be `str`, `int`, `float`, or `bool`.
    #[pyfunction]
    fn set_var(name: String, value: PyObjectRef, vm: &VirtualMachine) -> PyResult<()> {
        let dump = pyobject_to_yarn_value(&value, vm)?;
        match with_ctx(|ctx| ctx.set_yarn_var(&name, dump)) {
            Some(Ok(())) => Ok(()),
            Some(Err(err)) => Err(vm.new_runtime_error(err.as_string())),
            None => Err(vm.new_runtime_error("set_var: no API context installed".to_owned())),
        }
    }

    /// `get_var(name)` — read a Yarn dialog variable, or `None` if unset.
    /// **Quest hooks only**.
    #[pyfunction]
    fn get_var(name: String, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
        let outcome = with_ctx(|ctx| ctx.get_yarn_var(&name));
        match outcome {
            Some(Ok(Some(YarnValueDump::Number(n)))) => Ok(n.to_pyobject(vm)),
            Some(Ok(Some(YarnValueDump::String(s)))) => Ok(s.to_pyobject(vm)),
            Some(Ok(Some(YarnValueDump::Boolean(b)))) => Ok(b.to_pyobject(vm)),
            Some(Ok(None)) => Ok(vm.ctx.none()),
            Some(Err(err)) => Err(vm.new_runtime_error(err.as_string())),
            None => Err(vm.new_runtime_error("get_var: no API context installed".to_owned())),
        }
    }

    /// `complete_quest(quest_id)` — mark a quest finished successfully.
    /// **Quest hooks only**.
    #[pyfunction]
    fn complete_quest(quest_id: String, vm: &VirtualMachine) -> PyResult<()> {
        end_quest_inner(quest_id, false, vm)
    }

    /// `fail_quest(quest_id)` — mark a quest failed. **Quest hooks only**.
    #[pyfunction]
    fn fail_quest(quest_id: String, vm: &VirtualMachine) -> PyResult<()> {
        end_quest_inner(quest_id, true, vm)
    }

    fn end_quest_inner(quest_id: String, failed: bool, vm: &VirtualMachine) -> PyResult<()> {
        match with_ctx(|ctx| ctx.end_quest(&quest_id, failed)) {
            Some(Ok(())) => Ok(()),
            Some(Err(err)) => Err(vm.new_runtime_error(err.as_string())),
            None => Err(vm.new_runtime_error("end_quest: no API context installed".to_owned())),
        }
    }

    // --- world-mutating verbs ---------------------------------------------

    /// `spawn(type_id, x, y, z=0)` — spawn an object at the caller's
    /// current space. Raises `ValueError` if any of `type_id` / `x` / `y`
    /// is missing. The caller must have a space; in the headless admin
    /// REPL, `world.attach_player(id)` first.
    #[pyfunction]
    fn spawn(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        let type_id: String = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<String>(vm).ok())
            .ok_or_else(|| vm.new_value_error("spawn: type_id is required".to_owned()))?;
        let x: i32 = args
            .args
            .get(1)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .ok_or_else(|| vm.new_value_error("spawn: x is required".to_owned()))?;
        let y: i32 = args
            .args
            .get(2)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .ok_or_else(|| vm.new_value_error("spawn: y is required".to_owned()))?;
        let z: i32 = args
            .args
            .get(3)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .unwrap_or(0);
        queue(
            vm,
            GameCommand::AdminSpawn {
                type_id,
                tile_position: TilePosition::new(x, y, z),
            },
        )
    }

    /// Legacy alias for `world.spawn(...)` — matches the original
    /// `mud_api.spawn_object` signature.
    #[pyfunction]
    fn spawn_object(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        spawn(args, vm)
    }

    /// `despawn(object_id)` — remove a world object by id.
    #[pyfunction]
    fn despawn(object_id: i64, vm: &VirtualMachine) -> PyResult<()> {
        if object_id < 0 {
            return Err(vm.new_value_error("despawn: object_id must be >= 0".to_owned()));
        }
        queue(
            vm,
            GameCommand::AdminDespawn {
                object_id: object_id as u64,
            },
        )
    }

    /// `teleport(x, y, z=0, space_id=None)` — move the caller to a tile.
    /// Pass `space_id` to teleport across spaces (otherwise stays in the
    /// current space). **Admin-only verb.**
    #[pyfunction]
    fn teleport(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        let x: i32 = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .ok_or_else(|| vm.new_value_error("teleport: x is required".to_owned()))?;
        let y: i32 = args
            .args
            .get(1)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .ok_or_else(|| vm.new_value_error("teleport: y is required".to_owned()))?;
        let z: i32 = args
            .args
            .get(2)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .unwrap_or(0);
        let space_id_opt: Option<u64> = args
            .args
            .get(3)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .filter(|n| *n >= 0)
            .map(|n| n as u64);
        queue(
            vm,
            GameCommand::AdminTeleport {
                space_id: space_id_opt.map(SpaceId),
                tile_position: TilePosition::new(x, y, z),
            },
        )
    }

    /// `set_vitals(health=None, mana=None)` — clamp the caller's health
    /// and/or mana to a specific value. Pass `None` to leave a vital
    /// unchanged; raises `ValueError` if both are `None`. **Admin-only.**
    #[pyfunction]
    fn set_vitals(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        let health = args.args.first().and_then(coerce_optional_f32(vm));
        let mana = args.args.get(1).and_then(coerce_optional_f32(vm));
        if health.is_none() && mana.is_none() {
            return Err(vm.new_value_error(
                "set_vitals: at least one of health/mana must be provided".to_owned(),
            ));
        }
        queue(vm, GameCommand::AdminSetVitals { health, mana })
    }

    /// `set_object_state(object_id, state)` — overwrite an interactive
    /// object's state string (e.g. `"open"` / `"closed"` for a door).
    #[pyfunction]
    fn set_object_state(object_id: i64, state: String, vm: &VirtualMachine) -> PyResult<()> {
        if object_id < 0 {
            return Err(vm.new_value_error("set_object_state: object_id must be >= 0".to_owned()));
        }
        queue(
            vm,
            GameCommand::AdminSetObjectState {
                object_id: object_id as u64,
                state,
            },
        )
    }

    /// `rotate(object_id, direction)` — rotate a rotatable object.
    /// `direction` accepts `"cw"` / `"clockwise"` and `"ccw"` /
    /// `"counterclockwise"` / `"counter_clockwise"`.
    #[pyfunction]
    fn rotate(object_id: i64, direction: String, vm: &VirtualMachine) -> PyResult<()> {
        if object_id < 0 {
            return Err(vm.new_value_error("rotate: object_id must be >= 0".to_owned()));
        }
        let rotation = match direction.to_ascii_lowercase().as_str() {
            "cw" | "clockwise" => RotationDirection::Clockwise,
            "ccw" | "counterclockwise" | "counter_clockwise" => RotationDirection::CounterClockwise,
            other => {
                return Err(vm.new_value_error(format!(
                    "rotate: unknown direction '{other}' (expected cw/ccw)"
                )))
            }
        };
        queue(
            vm,
            GameCommand::RotateObject {
                object_id: object_id as u64,
                rotation,
            },
        )
    }

    /// `interact(object_id, verb)` — trigger a named interaction verb on
    /// an object (defined in its `metadata.yaml`). Common verbs include
    /// `"use"`, `"loot"`, `"talk"` — depends on the object.
    #[pyfunction]
    fn interact(object_id: i64, verb: String, vm: &VirtualMachine) -> PyResult<()> {
        if object_id < 0 {
            return Err(vm.new_value_error("interact: object_id must be >= 0".to_owned()));
        }
        queue(
            vm,
            GameCommand::InteractWithObject {
                object_id: object_id as u64,
                verb,
            },
        )
    }

    /// `open_container(object_id)` — open a container (chest, barrel, etc.)
    /// as the caller. The container UI surface is delivered via UI events.
    #[pyfunction]
    fn open_container(object_id: i64, vm: &VirtualMachine) -> PyResult<()> {
        if object_id < 0 {
            return Err(vm.new_value_error("open_container: object_id must be >= 0".to_owned()));
        }
        queue(
            vm,
            GameCommand::OpenContainer {
                object_id: object_id as u64,
            },
        )
    }

    /// `set_combat_target(object_id=None)` — point the caller's combat AI
    /// at `object_id`, or pass nothing / `None` to clear the target.
    #[pyfunction]
    fn set_combat_target(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        let target = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .filter(|n| *n >= 0)
            .map(|n| n as u64);
        queue(
            vm,
            GameCommand::SetCombatTarget {
                target_object_id: target,
            },
        )
    }

    /// `cast_spell(spell_id, target_object_id)` — cast a spell from
    /// `assets/spells/` at a target object. The caller pays the mana cost.
    #[pyfunction]
    fn cast_spell(spell_id: String, target_object_id: i64, vm: &VirtualMachine) -> PyResult<()> {
        if target_object_id < 0 {
            return Err(vm.new_value_error("cast_spell: target_object_id must be >= 0".to_owned()));
        }
        queue(
            vm,
            GameCommand::CastSpellAt {
                source: ItemReference::Slot(ItemSlotRef::Backpack(0)),
                spell_id,
                target_object_id: target_object_id as u64,
            },
        )
    }

    /// `move_item(from_slot, to_slot)` — convenience overload: move an
    /// item between two backpack slot indices. For equipment slots or
    /// container moves, push a `GameCommand::MoveItem` directly.
    #[pyfunction]
    fn move_item(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        let from: usize = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .filter(|n| *n >= 0)
            .map(|n| n as usize)
            .ok_or_else(|| vm.new_value_error("move_item: source slot is required".to_owned()))?;
        let to: usize = args
            .args
            .get(1)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .filter(|n| *n >= 0)
            .map(|n| n as usize)
            .ok_or_else(|| {
                vm.new_value_error("move_item: destination slot is required".to_owned())
            })?;
        queue(
            vm,
            GameCommand::MoveItem {
                source: ItemReference::Slot(ItemSlotRef::Backpack(from)),
                destination: ItemDestination::Slot(ItemSlotRef::Backpack(to)),
            },
        )
    }

    /// `set_floor(space_id, z, x, y, floor_type=None)` — overwrite a
    /// single floor tile. Pass `floor_type=None` to clear it back to the
    /// space's default. Powers the in-game map editor.
    #[pyfunction]
    fn set_floor(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        let space_id: u64 = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .filter(|n| *n >= 0)
            .map(|n| n as u64)
            .ok_or_else(|| vm.new_value_error("set_floor: space_id is required".to_owned()))?;
        let z: i32 = args
            .args
            .get(1)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .unwrap_or(0);
        let x: i32 = args
            .args
            .get(2)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .ok_or_else(|| vm.new_value_error("set_floor: x is required".to_owned()))?;
        let y: i32 = args
            .args
            .get(3)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n as i32)
            .ok_or_else(|| vm.new_value_error("set_floor: y is required".to_owned()))?;
        let floor_type: Option<FloorTypeId> = args
            .args
            .get(4)
            .and_then(|v| v.clone().try_into_value::<String>(vm).ok())
            .filter(|s| !s.is_empty());
        queue(
            vm,
            GameCommand::EditorSetFloorTile {
                space_id: SpaceId(space_id),
                z,
                x,
                y,
                floor_type,
            },
        )
    }

    /// `reset()` — clear the persistent Python scope, so the next input
    /// starts fresh (globals reset, imports reloaded). **Admin-only**, and
    /// only meaningful in the in-game `~` console.
    #[pyfunction]
    fn reset(vm: &VirtualMachine) -> PyResult<()> {
        match with_ctx(|ctx| ctx.reset_scope()) {
            Some(Ok(())) => Ok(()),
            Some(Err(err)) => Err(vm.new_runtime_error(err.as_string())),
            None => Err(vm.new_runtime_error("reset: no API context installed".to_owned())),
        }
    }

    /// `attach_player(player_id)` — **admin REPL only**. Bind this socket
    /// session's caller to a live player so subsequent `world.*` verbs
    /// run as that account (e.g. `world.give`, `world.player()`,
    /// `world.spawn`). Pass `None` to detach. Pre-headless contexts (the
    /// in-game console, quest hooks) reject this with `RuntimeError`.
    #[pyfunction]
    fn attach_player(args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
        let player_id: Option<u64> = match args.args.first() {
            None => None,
            Some(value) if vm.is_none(value) => None,
            Some(value) => match value.clone().try_into_value::<i64>(vm) {
                Ok(n) if n >= 0 => Some(n as u64),
                Ok(n) => {
                    return Err(vm.new_value_error(format!(
                        "attach_player: player id must be >= 0 (got {n})"
                    )))
                }
                Err(_) => {
                    return Err(
                        vm.new_type_error("attach_player: expected an integer or None".to_owned())
                    )
                }
            },
        };
        match with_ctx(|ctx| ctx.attach_player(player_id)) {
            Some(Ok(())) => Ok(()),
            Some(Err(err)) => Err(vm.new_runtime_error(err.as_string())),
            None => Err(vm.new_runtime_error("attach_player: no API context installed".to_owned())),
        }
    }

    // --- legacy compatibility shims for the original mud_api surface ------

    /// Legacy: pretty-printed `list[str]` of all world objects, formatted
    /// as `"id=N type=foo pos=(x, y)"`. Prefer `world.objects()` which
    /// returns structured dicts.
    #[pyfunction]
    fn list_objects(vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(vm.ctx.new_list(Vec::new()).into(), |ctx| {
            let lines: Vec<PyObjectRef> = ctx
                .snapshot()
                .objects
                .iter()
                .map(|object| {
                    format!(
                        "id={} type={} pos=({}, {})",
                        object.object_id, object.type_id, object.x, object.y
                    )
                    .to_pyobject(vm)
                })
                .collect();
            vm.ctx.new_list(lines).into()
        })
    }

    /// Legacy alias for `world.object_types()`.
    #[pyfunction]
    fn list_object_types(vm: &VirtualMachine) -> PyObjectRef {
        object_types(vm)
    }

    /// Legacy: caller's `(x, y)` tile position as a tuple. Prefer
    /// `world.player()` which returns a richer dict including `z`,
    /// `space_id`, `vitals`, and `facing`.
    #[pyfunction]
    fn player_position(vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(
            vm.ctx
                .new_tuple(vec![0i32.to_pyobject(vm), 0i32.to_pyobject(vm)])
                .into(),
            |ctx| {
                let snapshot = ctx.snapshot();
                let pos = ctx
                    .caller_player_id()
                    .and_then(|id| snapshot.players.iter().find(|p| p.player_id == id))
                    .map(|p| (p.x, p.y))
                    .unwrap_or((0, 0));
                vm.ctx
                    .new_tuple(vec![pos.0.to_pyobject(vm), pos.1.to_pyobject(vm)])
                    .into()
            },
        )
    }

    // --- Player class -----------------------------------------------------

    /// `Player` — handle to a live player. Returned by `world.player()`,
    /// `world.players()`, and `world.find_player(id)`.
    ///
    /// Read-only properties (`p.level`, `p.skills`, `p.attributes`,
    /// `p.skill_points`, `p.name`, `p.class_name`, `p.vitals`, `p.position`,
    /// ...) reflect the world snapshot built at the start of the current
    /// REPL input. They do **not** refresh after a mutation inside the
    /// same expression — for fresh reads, press Enter again. Subscript
    /// access (`p["x"]`, `p["vitals"]`) is supported for back-compat with
    /// the old dict-returning API.
    ///
    /// Mutation methods (`p.grant_xp`, `p.set_skill`, `p.full_heal`, ...)
    /// queue commands targeted at this player's id, independent of the
    /// session's attached caller — so `world.find_player(7).grant_xp(100)`
    /// works without any `attach_player` setup.
    #[pyattr]
    #[pyclass(name = "Player")]
    #[derive(Debug, PyPayload)]
    pub struct PyPlayer {
        pub player_id: u64,
    }

    #[pyclass]
    impl PyPlayer {
        // --- read-only getters ---

        #[pygetset]
        fn id(&self) -> u64 {
            self.player_id
        }

        #[pygetset]
        fn name(&self, vm: &VirtualMachine) -> PyResult<String> {
            view(self.player_id, vm, |p| p.display_name.clone())
        }

        #[pygetset]
        fn class_name(&self, vm: &VirtualMachine) -> PyResult<String> {
            view(self.player_id, vm, |p| p.class_label.clone())
        }

        #[pygetset]
        fn level(&self, vm: &VirtualMachine) -> PyResult<u32> {
            view(self.player_id, vm, |p| p.level)
        }

        #[pygetset]
        fn xp(&self, vm: &VirtualMachine) -> PyResult<u64> {
            view(self.player_id, vm, |p| p.current_xp)
        }

        #[pygetset]
        fn xp_for_next(&self, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
            view(self.player_id, vm, |p| match p.xp_for_next {
                Some(n) => n.to_pyobject(vm),
                None => vm.ctx.none(),
            })
        }

        #[pygetset]
        fn skill_points(&self, vm: &VirtualMachine) -> PyResult<u32> {
            view(self.player_id, vm, |p| p.available_skill_points)
        }

        #[pygetset]
        fn skills(&self, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
            view(self.player_id, vm, |p| skill_ranks_to_dict(&p.skill_ranks, vm))
        }

        #[pygetset]
        fn attributes(&self, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
            view(self.player_id, vm, |p| attribute_map_to_dict(&p.attributes, vm))
        }

        #[pygetset]
        fn vitals(&self, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
            view(self.player_id, vm, |p| vitals_to_dict(&p.vitals, vm))
        }

        #[pygetset]
        fn space_id(&self, vm: &VirtualMachine) -> PyResult<u64> {
            view(self.player_id, vm, |p| p.space_id)
        }

        #[pygetset]
        fn x(&self, vm: &VirtualMachine) -> PyResult<i32> {
            view(self.player_id, vm, |p| p.x)
        }

        #[pygetset]
        fn y(&self, vm: &VirtualMachine) -> PyResult<i32> {
            view(self.player_id, vm, |p| p.y)
        }

        #[pygetset]
        fn z(&self, vm: &VirtualMachine) -> PyResult<i32> {
            view(self.player_id, vm, |p| p.z)
        }

        #[pygetset]
        fn position(&self, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
            view(self.player_id, vm, |p| {
                vm.ctx
                    .new_tuple(vec![
                        p.x.to_pyobject(vm),
                        p.y.to_pyobject(vm),
                        p.z.to_pyobject(vm),
                    ])
                    .into()
            })
        }

        #[pygetset]
        fn facing(&self, vm: &VirtualMachine) -> PyResult<String> {
            view(self.player_id, vm, |p| p.facing.clone())
        }

        #[pygetset]
        fn object_id(&self, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
            view(self.player_id, vm, |p| match p.object_id {
                Some(id) => id.to_pyobject(vm),
                None => vm.ctx.none(),
            })
        }

        // --- mutation methods (admin-only, targeted) ---

        /// `grant_xp(amount)` — add raw XP, routed through the canonical
        /// XP pipeline so level-ups fire normally (events, toasts, skill
        /// points). `amount` must be non-negative.
        #[pymethod]
        fn grant_xp(&self, amount: i64, vm: &VirtualMachine) -> PyResult<()> {
            if amount < 0 {
                return Err(vm.new_value_error("grant_xp: amount must be >= 0".to_owned()));
            }
            queue_for(
                self.player_id,
                GameCommand::AdminGrantXp {
                    amount: amount as u64,
                },
                vm,
            )
        }

        /// `set_level(level)` — hard-set this player's level to a value in
        /// 1..=LEVEL_CAP. Awards skill points for every level crossed
        /// upward; downward changes do not refund anything.
        #[pymethod]
        fn set_level(&self, level: i64, vm: &VirtualMachine) -> PyResult<()> {
            if level < 1 || (level as u32) > LEVEL_CAP {
                return Err(vm.new_value_error(format!(
                    "set_level: level must be in 1..={LEVEL_CAP}"
                )));
            }
            queue_for(
                self.player_id,
                GameCommand::AdminSetLevel {
                    level: level as u32,
                },
                vm,
            )
        }

        /// `grant_skill_points(amount)` — increase the player's unspent
        /// skill-point pool. `amount` must be non-negative.
        #[pymethod]
        fn grant_skill_points(&self, amount: i64, vm: &VirtualMachine) -> PyResult<()> {
            if amount < 0 {
                return Err(
                    vm.new_value_error("grant_skill_points: amount must be >= 0".to_owned())
                );
            }
            queue_for(
                self.player_id,
                GameCommand::AdminGrantSkillPoints {
                    amount: amount as u32,
                },
                vm,
            )
        }

        /// `set_skill(name, rank)` — overwrite a single skill's rank,
        /// bypassing the class/level cap and point cost. `name` is the
        /// canonical label, case-insensitive (`"Thievery"`, `"Stealth"`,
        /// ...). `rank` is clamped to 0..=255.
        #[pymethod]
        fn set_skill(&self, name: String, rank: i64, vm: &VirtualMachine) -> PyResult<()> {
            let skill = Skill::from_label(&name)
                .ok_or_else(|| vm.new_value_error(format!("set_skill: unknown skill '{name}'")))?;
            if !(0..=255).contains(&rank) {
                return Err(vm.new_value_error("set_skill: rank must be in 0..=255".to_owned()));
            }
            queue_for(
                self.player_id,
                GameCommand::AdminSetSkillRank {
                    skill,
                    rank: rank as u8,
                },
                vm,
            )
        }

        /// `set_attribute(name, value)` — overwrite a single attribute on
        /// `BaseStats.attributes`. Bypasses point-buy validation; the
        /// next frame's `refresh_derived_player_stats` reclamps derived
        /// stats. `name` accepts full labels or short aliases
        /// (`"agility"`/`"agi"`/`"dex"`, etc.).
        #[pymethod]
        fn set_attribute(&self, name: String, value: i64, vm: &VirtualMachine) -> PyResult<()> {
            let kind = AttributeKind::from_label(&name).ok_or_else(|| {
                vm.new_value_error(format!("set_attribute: unknown attribute '{name}'"))
            })?;
            if !(i32::MIN as i64..=i32::MAX as i64).contains(&value) {
                return Err(vm.new_value_error("set_attribute: value out of i32 range".to_owned()));
            }
            queue_for(
                self.player_id,
                GameCommand::AdminSetAttribute {
                    kind,
                    value: value as i32,
                },
                vm,
            )
        }

        /// `set_class(name)` — switch the player's class. Does not
        /// redistribute skill ranks. `name` matches a `Class` label
        /// case-insensitively (`"Fighter"`, `"Wizard"`, `"Cleric"`,
        /// `"Vagabond"`).
        #[pymethod]
        fn set_class(&self, name: String, vm: &VirtualMachine) -> PyResult<()> {
            let class = Class::from_label(&name)
                .ok_or_else(|| vm.new_value_error(format!("set_class: unknown class '{name}'")))?;
            queue_for(self.player_id, GameCommand::AdminSetClass { class }, vm)
        }

        /// `full_heal()` — restore health and mana to their respective
        /// maxes.
        #[pymethod]
        fn full_heal(&self, vm: &VirtualMachine) -> PyResult<()> {
            queue_for(self.player_id, GameCommand::AdminFullHeal, vm)
        }

        /// `set_vitals(health=None, mana=None)` — clamp this player's
        /// health and/or mana directly. Each `None` leaves that vital
        /// alone; raises `ValueError` when both are `None`.
        #[pymethod]
        fn set_vitals(&self, args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
            let health = args.args.first().and_then(coerce_optional_f32(vm));
            let mana = args.args.get(1).and_then(coerce_optional_f32(vm));
            if health.is_none() && mana.is_none() {
                return Err(vm.new_value_error(
                    "set_vitals: at least one of health/mana must be provided".to_owned(),
                ));
            }
            queue_for(
                self.player_id,
                GameCommand::AdminSetVitals { health, mana },
                vm,
            )
        }

        /// `teleport(x, y, z=0, space_id=None)` — move the player to a
        /// tile. Pass `space_id` to teleport across spaces.
        #[pymethod]
        fn teleport(&self, args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
            let x: i32 = args
                .args
                .first()
                .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
                .map(|n| n as i32)
                .ok_or_else(|| vm.new_value_error("teleport: x is required".to_owned()))?;
            let y: i32 = args
                .args
                .get(1)
                .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
                .map(|n| n as i32)
                .ok_or_else(|| vm.new_value_error("teleport: y is required".to_owned()))?;
            let z: i32 = args
                .args
                .get(2)
                .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
                .map(|n| n as i32)
                .unwrap_or(0);
            let space_id_opt: Option<u64> = args
                .args
                .get(3)
                .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
                .filter(|n| *n >= 0)
                .map(|n| n as u64);
            queue_for(
                self.player_id,
                GameCommand::AdminTeleport {
                    space_id: space_id_opt.map(SpaceId),
                    tile_position: TilePosition::new(x, y, z),
                },
                vm,
            )
        }

        /// `give(type_id, count=1)` — give items to this player's
        /// inventory.
        #[pymethod]
        fn give(&self, args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
            let (type_id, count) = parse_item_count(&args, 1, vm);
            if type_id.is_empty() {
                return Err(vm.new_value_error("give: type_id is required".to_owned()));
            }
            queue_for(self.player_id, GameCommand::GiveItem { type_id, count }, vm)
        }

        /// `take(type_id, count=1)` — remove items from this player's
        /// inventory. No-op when the player has fewer than `count`.
        #[pymethod]
        fn take(&self, args: FuncArgs, vm: &VirtualMachine) -> PyResult<()> {
            let (type_id, count) = parse_item_count(&args, 1, vm);
            if type_id.is_empty() {
                return Err(vm.new_value_error("take: type_id is required".to_owned()));
            }
            queue_for(self.player_id, GameCommand::TakeItem { type_id, count }, vm)
        }

        // --- dunders ---

        #[pymethod(magic)]
        fn repr(&self, vm: &VirtualMachine) -> PyResult<String> {
            view(self.player_id, vm, |p| {
                let xp = match p.xp_for_next {
                    Some(n) => format!("{}/{} XP", p.xp_into_level, n),
                    None => format!("{} XP (capped)", p.current_xp),
                };
                format!(
                    "<Player id={} '{}' {} L{} {}, {} SP>",
                    p.player_id,
                    p.display_name,
                    p.class_label,
                    p.level,
                    xp,
                    p.available_skill_points,
                )
            })
        }

        /// Subscript access for back-compat with the old dict-returning
        /// `world.player()` API. Supports keys: `id`, `object_id`,
        /// `space_id`, `x`, `y`, `z`, `vitals`, `facing`, `name`, `class`,
        /// `level`, `xp`, `xp_for_next`, `attributes`, `skills`,
        /// `skill_points`.
        #[pymethod(magic)]
        fn getitem(&self, key: String, vm: &VirtualMachine) -> PyResult<PyObjectRef> {
            view(self.player_id, vm, |p| -> PyResult<PyObjectRef> {
                match key.as_str() {
                    "id" => Ok(p.player_id.to_pyobject(vm)),
                    "object_id" => Ok(match p.object_id {
                        Some(id) => id.to_pyobject(vm),
                        None => vm.ctx.none(),
                    }),
                    "space_id" => Ok(p.space_id.to_pyobject(vm)),
                    "x" => Ok(p.x.to_pyobject(vm)),
                    "y" => Ok(p.y.to_pyobject(vm)),
                    "z" => Ok(p.z.to_pyobject(vm)),
                    "vitals" => Ok(vitals_to_dict(&p.vitals, vm)),
                    "facing" => Ok(p.facing.clone().to_pyobject(vm)),
                    "name" => Ok(p.display_name.clone().to_pyobject(vm)),
                    "class" | "class_name" => Ok(p.class_label.clone().to_pyobject(vm)),
                    "level" => Ok(p.level.to_pyobject(vm)),
                    "xp" => Ok(p.current_xp.to_pyobject(vm)),
                    "xp_for_next" => Ok(match p.xp_for_next {
                        Some(n) => n.to_pyobject(vm),
                        None => vm.ctx.none(),
                    }),
                    "attributes" => Ok(attribute_map_to_dict(&p.attributes, vm)),
                    "skills" => Ok(skill_ranks_to_dict(&p.skill_ranks, vm)),
                    "skill_points" => Ok(p.available_skill_points.to_pyobject(vm)),
                    other => Err(vm.new_key_error(other.to_owned().to_pyobject(vm))),
                }
            })?
        }
    }

    /// Look up `player_id`'s `PlayerView` in the current snapshot and run
    /// `f` against it. Returns `LookupError` when the player is not in the
    /// snapshot (typically: disconnected or never connected), or
    /// `RuntimeError` when no API context is installed (programmer error).
    fn view<R>(
        player_id: u64,
        vm: &VirtualMachine,
        f: impl FnOnce(&PlayerView) -> R,
    ) -> PyResult<R> {
        let outcome = with_ctx(|ctx| {
            ctx.snapshot()
                .players
                .iter()
                .find(|p| p.player_id == player_id)
                .map(f)
                .ok_or_else(|| {
                    vm.new_lookup_error(format!(
                        "Player id={player_id} not found (disconnected?)"
                    ))
                })
        });
        match outcome {
            Some(Ok(value)) => Ok(value),
            Some(Err(err)) => Err(err),
            None => Err(vm.new_runtime_error("Player: no API context installed".to_owned())),
        }
    }

    /// Queue a command targeted explicitly at `player_id`, regardless of
    /// the session's attached caller.
    fn queue_for(player_id: u64, command: GameCommand, vm: &VirtualMachine) -> PyResult<()> {
        match with_ctx(|ctx| ctx.queue_command_for_player(PlayerId(player_id), command)) {
            Some(Ok(())) => Ok(()),
            Some(Err(err)) => Err(vm.new_runtime_error(err.as_string())),
            None => Err(vm.new_runtime_error("Player: no API context installed".to_owned())),
        }
    }

    // --- helpers ----------------------------------------------------------

    fn queue(vm: &VirtualMachine, command: GameCommand) -> PyResult<()> {
        match with_ctx(|ctx| ctx.queue_command(command)) {
            Some(Ok(())) => Ok(()),
            Some(Err(err)) => Err(vm.new_runtime_error(err.as_string())),
            None => Err(vm.new_runtime_error("world: no API context installed".to_owned())),
        }
    }

    fn parse_item_count(args: &FuncArgs, default_count: u32, vm: &VirtualMachine) -> (String, u32) {
        let type_id: String = args
            .args
            .first()
            .and_then(|v| v.clone().try_into_value::<String>(vm).ok())
            .unwrap_or_default();
        let count: u32 = args
            .args
            .get(1)
            .and_then(|v| v.clone().try_into_value::<i64>(vm).ok())
            .map(|n| n.max(0) as u32)
            .unwrap_or(default_count);
        (type_id, count)
    }

    fn coerce_optional_f32<'a>(
        vm: &'a VirtualMachine,
    ) -> impl Fn(&PyObjectRef) -> Option<f32> + 'a {
        move |value: &PyObjectRef| {
            if vm.is_none(value) {
                return None;
            }
            if let Ok(n) = value.clone().try_into_value::<f64>(vm) {
                return Some(n as f32);
            }
            if let Ok(n) = value.clone().try_into_value::<i64>(vm) {
                return Some(n as f32);
            }
            None
        }
    }

    // --- per-character stash ----------------------------------------------

    /// `stash_get(key)` — read a JSON value from the caller's
    /// `CharacterStash` snapshot, or `None` if unset. Returns the value as
    /// the closest Python equivalent: `dict` / `list` / `str` / `int` /
    /// `float` / `bool` / `None`.
    #[pyfunction]
    fn stash_get(key: String, vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(vm.ctx.none(), |ctx| match ctx.stash_get(&key) {
            Some(value) => json_to_pyobject(&value, vm),
            None => vm.ctx.none(),
        })
    }

    /// `stash_has(key)` — `True` when the caller's stash snapshot has the
    /// key. Useful for gating Python branches the same way `<<if
    /// stash_has("foo")>>` does in Yarn.
    #[pyfunction]
    fn stash_has(key: String) -> bool {
        with_ctx_or(false, |ctx| ctx.stash_has(&key))
    }

    /// `stash_set(key, value)` — write `value` into the caller's stash
    /// under `key`. The mutation is queued and applied next tick. `value`
    /// accepts `str`, `int`, `float`, `bool`, `None`, `list`, and `dict`
    /// (nested allowed).
    #[pyfunction]
    fn stash_set(key: String, value: PyObjectRef, vm: &VirtualMachine) -> PyResult<()> {
        let json = pyobject_to_json(&value, vm)?;
        match with_ctx(|ctx| ctx.stash_set(&key, Some(json))) {
            Some(Ok(())) => Ok(()),
            Some(Err(err)) => Err(vm.new_runtime_error(err.as_string())),
            None => Err(vm.new_runtime_error("stash_set: no API context installed".to_owned())),
        }
    }

    /// `log_write(subsection, title, body)` — append (or replace) an entry
    /// in the caller's `Quests` log section. Marks the entry as engine-owned:
    /// the player can't edit `body` from the UI, but can add a free-form
    /// `player_notes` tail under it. Use for quest journal narration —
    /// `world.log_write("demo_hunter", "Step 1", "Travel north")`.
    #[pyfunction]
    fn log_write(
        subsection: String,
        title: String,
        body: String,
        vm: &VirtualMachine,
    ) -> PyResult<()> {
        if subsection.is_empty() {
            return Err(vm.new_value_error("log_write: subsection is required".to_owned()));
        }
        queue(
            vm,
            GameCommand::UpsertLogEntry {
                section: crate::log::QUESTS_SECTION.to_owned(),
                subsection,
                title,
                body,
                owner: crate::log::LogOwner::Engine,
            },
        )
    }

    /// `stash_delete(key)` — remove a key from the caller's stash. No-op
    /// if the key isn't set.
    #[pyfunction]
    fn stash_delete(key: String, vm: &VirtualMachine) -> PyResult<()> {
        match with_ctx(|ctx| ctx.stash_set(&key, None)) {
            Some(Ok(())) => Ok(()),
            Some(Err(err)) => Err(vm.new_runtime_error(err.as_string())),
            None => Err(vm.new_runtime_error("stash_delete: no API context installed".to_owned())),
        }
    }

    fn pyobject_to_yarn_value(obj: &PyObjectRef, vm: &VirtualMachine) -> PyResult<YarnValueDump> {
        if let Ok(b) = obj.clone().try_into_value::<bool>(vm) {
            return Ok(YarnValueDump::Boolean(b));
        }
        if let Ok(n) = obj.clone().try_into_value::<i64>(vm) {
            return Ok(YarnValueDump::Number(n as f32));
        }
        if let Ok(n) = obj.clone().try_into_value::<f64>(vm) {
            return Ok(YarnValueDump::Number(n as f32));
        }
        if let Ok(s) = obj.clone().try_into_value::<String>(vm) {
            return Ok(YarnValueDump::String(s));
        }
        Err(vm.new_type_error("world: yarn vars must be str, int, float, or bool".to_owned()))
    }

    /// Recursively converts a Python value to `serde_json::Value`. Handles
    /// the common scalar types plus `list` and `dict`. Falls back to
    /// `repr(value)` for other types, since the stash is meant to hold
    /// plain JSON-shaped data.
    fn pyobject_to_json(obj: &PyObjectRef, vm: &VirtualMachine) -> PyResult<serde_json::Value> {
        use rustpython_vm::builtins::{PyDict, PyList};
        use rustpython_vm::AsObject;

        if vm.is_none(obj) {
            return Ok(serde_json::Value::Null);
        }
        if let Ok(b) = obj.clone().try_into_value::<bool>(vm) {
            // `try_into_value::<bool>` succeeds for ints too — guard by
            // checking that the object's class is exactly `bool`.
            if obj.class().is(vm.ctx.types.bool_type) {
                return Ok(serde_json::Value::Bool(b));
            }
        }
        if let Ok(n) = obj.clone().try_into_value::<i64>(vm) {
            return Ok(serde_json::Value::from(n));
        }
        if let Ok(n) = obj.clone().try_into_value::<f64>(vm) {
            return Ok(serde_json::Number::from_f64(n)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null));
        }
        if let Ok(s) = obj.clone().try_into_value::<String>(vm) {
            return Ok(serde_json::Value::String(s));
        }
        if let Some(list) = obj.downcast_ref::<PyList>() {
            let borrowed = list.borrow_vec();
            let mut out = Vec::with_capacity(borrowed.len());
            for item in borrowed.iter() {
                out.push(pyobject_to_json(item, vm)?);
            }
            return Ok(serde_json::Value::Array(out));
        }
        if let Some(dict) = obj.downcast_ref::<PyDict>() {
            let mut out = serde_json::Map::new();
            for (key_obj, value_obj) in dict {
                let key: String = key_obj.clone().try_into_value(vm).map_err(|_| {
                    vm.new_type_error("stash_set: dict keys must be strings".to_owned())
                })?;
                out.insert(key, pyobject_to_json(&value_obj, vm)?);
            }
            return Ok(serde_json::Value::Object(out));
        }
        Err(vm.new_type_error(
            "stash_set: value must be JSON-shaped (str/int/float/bool/None/list/dict)".to_owned(),
        ))
    }

    /// Inverse of `pyobject_to_json`. Used by `world.stash_get` to hand
    /// scripts a Python value they can pattern-match on directly.
    fn json_to_pyobject(value: &serde_json::Value, vm: &VirtualMachine) -> PyObjectRef {
        use rustpython_vm::builtins::PyDict;

        match value {
            serde_json::Value::Null => vm.ctx.none(),
            serde_json::Value::Bool(b) => b.to_pyobject(vm),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    i.to_pyobject(vm)
                } else if let Some(f) = n.as_f64() {
                    f.to_pyobject(vm)
                } else {
                    vm.ctx.none()
                }
            }
            serde_json::Value::String(s) => s.clone().to_pyobject(vm),
            serde_json::Value::Array(items) => {
                let py_items: Vec<PyObjectRef> =
                    items.iter().map(|v| json_to_pyobject(v, vm)).collect();
                vm.ctx.new_list(py_items).into()
            }
            serde_json::Value::Object(map) => {
                let dict = PyDict::new_ref(&vm.ctx);
                for (key, val) in map {
                    dict.set_item(key.as_str(), json_to_pyobject(val, vm), vm)
                        .ok();
                }
                dict.into()
            }
        }
    }
}
