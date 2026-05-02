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
use crate::scripting_api::snapshots::{object_to_dict, player_to_dict, space_to_dict};
use crate::scripting_api::{with_ctx, with_ctx_or};
use crate::world::components::{SpaceId, TilePosition};
use crate::world::floor_definitions::FloorTypeId;

const HELP_TEXT: &str = "\
world API cheat sheet — use help(world.<verb>) for details.

Read:
  world.now()                            world.is_admin()
  world.caller_player_id()               world.player()
  world.players()                        world.spaces()
  world.objects([space_id])              world.object(id)
  world.object_types()                   world.spell_ids()
  world.floor_tile(space_id, z, x, y)    world.player_has(type_id, count=1)

Write:
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
    use rustpython_vm::{PyObjectRef, PyResult, VirtualMachine};

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

    /// Snapshot dict for the current caller (`player_id`, `space_id`,
    /// `x` / `y` / `z`, `vitals`, `facing`), or `None` if no player is
    /// attached. In the headless admin REPL use `world.attach_player(id)`
    /// first.
    #[pyfunction]
    fn player(vm: &VirtualMachine) -> PyObjectRef {
        with_ctx_or(vm.ctx.none(), |ctx| {
            let snapshot = ctx.snapshot();
            let id = match ctx.caller_player_id() {
                Some(id) => id,
                None => return vm.ctx.none(),
            };
            match snapshot.players.iter().find(|p| p.player_id == id) {
                Some(p) => player_to_dict(p, vm),
                None => vm.ctx.none(),
            }
        })
    }

    /// Roster of players. In admin contexts: every connected player. In a
    /// quest hook: just the caller (for safety / encapsulation).
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
            let items = iter.map(|p| player_to_dict(p, vm)).collect();
            vm.ctx.new_list(items).into()
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
}
