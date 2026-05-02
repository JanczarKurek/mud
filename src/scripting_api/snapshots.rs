//! Plain-old-data views that capture authoritative game state at a point in
//! time. Both the admin console and the quest VM build a fresh `WorldSnapshot`
//! before invoking a Python hook; the shared `world` module reads from it.
//!
//! The structs here are deliberately simple (owned strings, integers,
//! `Option`s) so they convert cleanly into Python dicts/lists via
//! `to_py_dict` / `to_py_list_of_dicts`.

use std::collections::HashMap;

use rustpython_vm::builtins::PyDict;
use rustpython_vm::convert::ToPyObject;
use rustpython_vm::{PyObjectRef, VirtualMachine};

#[derive(Clone, Debug, Default)]
pub struct VitalsView {
    pub health: f32,
    pub max_health: f32,
    pub mana: f32,
    pub max_mana: f32,
}

#[derive(Clone, Debug)]
pub struct WorldObjectView {
    pub object_id: u64,
    pub type_id: String,
    pub space_id: u64,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub state: Option<String>,
    pub vitals: Option<VitalsView>,
    pub quantity: u32,
    pub facing: String,
    pub is_npc: bool,
    pub is_container: bool,
    pub is_movable: bool,
    pub is_rotatable: bool,
    pub has_dialog: bool,
}

#[derive(Clone, Debug)]
pub struct PlayerView {
    pub player_id: u64,
    pub object_id: Option<u64>,
    pub space_id: u64,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub vitals: VitalsView,
    pub facing: String,
}

#[derive(Clone, Debug)]
pub struct SpaceView {
    pub space_id: u64,
    pub authored_id: String,
    pub width: i32,
    pub height: i32,
    pub fill_floor_type: String,
}

#[derive(Clone, Debug, Default)]
pub struct FloorMapView {
    pub width: i32,
    pub height: i32,
    /// Row-major: index `y * width + x`. `None` = empty tile.
    pub tiles: Vec<Option<String>>,
}

impl FloorMapView {
    pub fn get(&self, x: i32, y: i32) -> Option<&str> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        self.tiles
            .get((y * self.width + x) as usize)
            .and_then(|opt| opt.as_deref())
    }
}

#[derive(Clone, Debug, Default)]
pub struct WorldSnapshot {
    pub world_time: f32,
    pub object_types: Vec<String>,
    pub spell_ids: Vec<String>,
    pub spaces: Vec<SpaceView>,
    pub objects: Vec<WorldObjectView>,
    pub players: Vec<PlayerView>,
    /// Keyed by `(space_id, z)`.
    pub floor_maps: HashMap<(u64, i32), FloorMapView>,
    pub local_player_id: Option<u64>,
    pub local_player_space_id: Option<u64>,
    /// Acting player's current inventory as `{type_id -> total quantity}`.
    /// Only populated for the caller in quest contexts; admin contexts may
    /// leave it empty (admin verbs don't currently need it).
    pub caller_inventory: HashMap<String, u32>,
}

pub fn vitals_to_dict(vitals: &VitalsView, vm: &VirtualMachine) -> PyObjectRef {
    let dict = PyDict::new_ref(&vm.ctx);
    dict.set_item("health", vitals.health.to_pyobject(vm), vm)
        .ok();
    dict.set_item("max_health", vitals.max_health.to_pyobject(vm), vm)
        .ok();
    dict.set_item("mana", vitals.mana.to_pyobject(vm), vm).ok();
    dict.set_item("max_mana", vitals.max_mana.to_pyobject(vm), vm)
        .ok();
    dict.into()
}

pub fn object_to_dict(object: &WorldObjectView, vm: &VirtualMachine) -> PyObjectRef {
    let dict = PyDict::new_ref(&vm.ctx);
    dict.set_item("id", object.object_id.to_pyobject(vm), vm)
        .ok();
    dict.set_item("type_id", object.type_id.clone().to_pyobject(vm), vm)
        .ok();
    dict.set_item("space_id", object.space_id.to_pyobject(vm), vm)
        .ok();
    dict.set_item("x", object.x.to_pyobject(vm), vm).ok();
    dict.set_item("y", object.y.to_pyobject(vm), vm).ok();
    dict.set_item("z", object.z.to_pyobject(vm), vm).ok();
    let state_value: PyObjectRef = match &object.state {
        Some(s) => s.clone().to_pyobject(vm),
        None => vm.ctx.none(),
    };
    dict.set_item("state", state_value, vm).ok();
    let vitals_value: PyObjectRef = match &object.vitals {
        Some(v) => vitals_to_dict(v, vm),
        None => vm.ctx.none(),
    };
    dict.set_item("vitals", vitals_value, vm).ok();
    dict.set_item("quantity", object.quantity.to_pyobject(vm), vm)
        .ok();
    dict.set_item("facing", object.facing.clone().to_pyobject(vm), vm)
        .ok();
    dict.set_item("is_npc", object.is_npc.to_pyobject(vm), vm)
        .ok();
    dict.set_item("is_container", object.is_container.to_pyobject(vm), vm)
        .ok();
    dict.set_item("is_movable", object.is_movable.to_pyobject(vm), vm)
        .ok();
    dict.set_item("is_rotatable", object.is_rotatable.to_pyobject(vm), vm)
        .ok();
    dict.set_item("has_dialog", object.has_dialog.to_pyobject(vm), vm)
        .ok();
    dict.into()
}

pub fn player_to_dict(player: &PlayerView, vm: &VirtualMachine) -> PyObjectRef {
    let dict = PyDict::new_ref(&vm.ctx);
    dict.set_item("id", player.player_id.to_pyobject(vm), vm)
        .ok();
    let object_id_value: PyObjectRef = match player.object_id {
        Some(id) => id.to_pyobject(vm),
        None => vm.ctx.none(),
    };
    dict.set_item("object_id", object_id_value, vm).ok();
    dict.set_item("space_id", player.space_id.to_pyobject(vm), vm)
        .ok();
    dict.set_item("x", player.x.to_pyobject(vm), vm).ok();
    dict.set_item("y", player.y.to_pyobject(vm), vm).ok();
    dict.set_item("z", player.z.to_pyobject(vm), vm).ok();
    dict.set_item("vitals", vitals_to_dict(&player.vitals, vm), vm)
        .ok();
    dict.set_item("facing", player.facing.clone().to_pyobject(vm), vm)
        .ok();
    dict.into()
}

pub fn space_to_dict(space: &SpaceView, vm: &VirtualMachine) -> PyObjectRef {
    let dict = PyDict::new_ref(&vm.ctx);
    dict.set_item("id", space.space_id.to_pyobject(vm), vm).ok();
    dict.set_item("authored_id", space.authored_id.clone().to_pyobject(vm), vm)
        .ok();
    dict.set_item("width", space.width.to_pyobject(vm), vm).ok();
    dict.set_item("height", space.height.to_pyobject(vm), vm)
        .ok();
    dict.set_item(
        "fill_floor_type",
        space.fill_floor_type.clone().to_pyobject(vm),
        vm,
    )
    .ok();
    dict.into()
}
