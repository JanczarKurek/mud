//! Build a [`WorldSnapshot`] from authoritative Bevy state.
//!
//! Both the admin console (per `Enter` press) and the quest engine (per
//! Python hook invocation) need a fresh read-only view of the world to
//! pass into the `world` Python module. The work is identical; this
//! module exposes a single [`WorldSnapshotParams`] `SystemParam` that
//! bundles every resource/query the snapshot consumes, so callers can
//! request one parameter and call `.build_for_player(...)`.

use std::collections::HashMap;

use bevy::ecs::query::Has;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::crafting::CharacterStash;
use crate::dialog::components::DialogNode;
use crate::magic::resources::SpellDefinitions;
use crate::npc::components::Npc;
use crate::player::classes::Class;
use crate::player::components::{
    BaseStats, Inventory, Player, PlayerId, PlayerIdentity, VitalStats,
};
use crate::player::progression::Experience;
use crate::player::skills::{Skill, SkillSheet};
use crate::scripting_api::snapshots::{
    AttributeMap, FloorMapView, PlayerView, SpaceView, VitalsView, WorldObjectView, WorldSnapshot,
};
use crate::world::components::{
    Container, Facing, Movable, ObjectState, OverworldObject, Quantity, Rotatable, SpaceResident,
    TilePosition,
};
use crate::world::floor_map::FloorMaps;
use crate::world::lighting::WorldClock;
use crate::world::object_definitions::OverworldObjectDefinitions;
use crate::world::resources::SpaceManager;

/// Bundle of read-only ECS handles needed to materialise a `WorldSnapshot`.
/// Bevy caps individual system parameter counts, so we group them here.
#[derive(SystemParam)]
pub struct WorldSnapshotParams<'w, 's> {
    pub object_definitions: Res<'w, OverworldObjectDefinitions>,
    pub spell_definitions: Res<'w, SpellDefinitions>,
    pub space_manager: Res<'w, SpaceManager>,
    pub floor_maps: Res<'w, FloorMaps>,
    pub world_clock: Res<'w, WorldClock>,
    pub player_query: Query<
        'w,
        's,
        (
            &'static PlayerIdentity,
            &'static SpaceResident,
            &'static TilePosition,
            &'static VitalStats,
            &'static Inventory,
            Option<&'static Facing>,
            &'static OverworldObject,
            Option<&'static CharacterStash>,
            &'static Experience,
            &'static SkillSheet,
            &'static BaseStats,
            &'static Class,
        ),
        With<Player>,
    >,
    pub object_query: Query<
        'w,
        's,
        (
            &'static OverworldObject,
            &'static SpaceResident,
            &'static TilePosition,
            Option<&'static ObjectState>,
            Option<&'static VitalStats>,
            Option<&'static Quantity>,
            Option<&'static Facing>,
            Has<Container>,
            Has<Npc>,
            Has<Movable>,
            Has<Rotatable>,
            Has<DialogNode>,
        ),
        Without<Player>,
    >,
}

impl<'w, 's> WorldSnapshotParams<'w, 's> {
    /// Snapshot the world from the perspective of `caller`. When
    /// `caller` is `None` the snapshot's `local_player_id` and
    /// `caller_inventory` are left empty (admin console with no joined
    /// player); otherwise the matching player's inventory is captured
    /// for `world.player_has`/`world.give`/etc.
    pub fn build_for_player(&self, caller: Option<PlayerId>) -> WorldSnapshot {
        let object_types: Vec<String> = self.object_definitions.ids().map(str::to_owned).collect();

        let spell_ids: Vec<String> = self.spell_definitions.ids().map(str::to_owned).collect();

        let spaces: Vec<SpaceView> = self
            .space_manager
            .spaces
            .values()
            .map(|space| SpaceView {
                space_id: space.id.0,
                authored_id: space.authored_id.clone(),
                width: space.width,
                height: space.height,
                fill_floor_type: space.fill_floor_type.clone(),
            })
            .collect();

        let mut floor_maps: HashMap<(u64, i32), FloorMapView> = HashMap::new();
        for (space_id, z, map) in self.floor_maps.iter() {
            floor_maps.insert(
                (space_id.0, z),
                FloorMapView {
                    width: map.width,
                    height: map.height,
                    tiles: map.tiles.clone(),
                },
            );
        }

        let mut players: Vec<PlayerView> = Vec::new();
        let mut local_player_id: Option<u64> = None;
        let mut local_player_space: Option<u64> = None;
        let mut caller_inventory: HashMap<String, u32> = HashMap::new();
        let mut caller_stash: HashMap<String, serde_json::Value> = HashMap::new();

        for (
            identity,
            resident,
            tile,
            vitals,
            inventory,
            facing,
            player_object,
            stash,
            experience,
            skill_sheet,
            base_stats,
            class,
        ) in self.player_query.iter()
        {
            let attrs = &base_stats.attributes;
            let view = PlayerView {
                player_id: identity.id.0,
                object_id: Some(player_object.object_id),
                space_id: resident.space_id.0,
                x: tile.x,
                y: tile.y,
                z: tile.z,
                vitals: VitalsView {
                    health: vitals.health,
                    max_health: vitals.max_health,
                    mana: vitals.mana,
                    max_mana: vitals.max_mana,
                },
                facing: format!("{:?}", facing.copied().unwrap_or_default().0),
                display_name: identity.display_name.clone(),
                class_label: class.label().to_owned(),
                level: experience.level,
                current_xp: experience.current_xp,
                xp_into_level: experience.xp_into_level(),
                xp_for_next: experience.xp_for_next(),
                attributes: AttributeMap {
                    strength: attrs.strength,
                    agility: attrs.agility,
                    constitution: attrs.constitution,
                    willpower: attrs.willpower,
                    charisma: attrs.charisma,
                    focus: attrs.focus,
                },
                skill_ranks: Skill::ALL
                    .iter()
                    .map(|skill| (skill.label().to_owned(), skill_sheet.rank(*skill)))
                    .collect(),
                available_skill_points: skill_sheet.available_points,
            };

            let is_caller = match caller {
                Some(target) => identity.id == target,
                None => local_player_id.is_none(),
            };
            if is_caller {
                local_player_id = Some(identity.id.0);
                local_player_space = Some(resident.space_id.0);
                caller_inventory = collect_inventory_counts(inventory);
                if let Some(stash) = stash {
                    caller_stash = stash.entries.clone();
                }
            }

            players.push(view);
        }

        let mut objects: Vec<WorldObjectView> = Vec::new();
        for (
            object,
            resident,
            tile,
            state,
            vitals,
            quantity,
            facing,
            has_container,
            has_npc,
            has_movable,
            has_rotatable,
            has_dialog,
        ) in self.object_query.iter()
        {
            objects.push(WorldObjectView {
                object_id: object.object_id,
                type_id: object.definition_id.clone(),
                space_id: resident.space_id.0,
                x: tile.x,
                y: tile.y,
                z: tile.z,
                state: state.map(|s| s.0.clone()),
                vitals: vitals.map(|v| VitalsView {
                    health: v.health,
                    max_health: v.max_health,
                    mana: v.mana,
                    max_mana: v.max_mana,
                }),
                quantity: quantity.map(|q| q.0).unwrap_or(1),
                facing: format!("{:?}", facing.copied().unwrap_or_default().0),
                is_npc: has_npc,
                is_container: has_container,
                is_movable: has_movable,
                is_rotatable: has_rotatable,
                has_dialog,
            });
        }

        WorldSnapshot {
            world_time: self.world_clock.time_of_day,
            object_types,
            spell_ids,
            spaces,
            objects,
            players,
            floor_maps,
            local_player_id,
            local_player_space_id: local_player_space,
            caller_inventory,
            caller_stash,
        }
    }
}

fn collect_inventory_counts(inventory: &Inventory) -> HashMap<String, u32> {
    let mut totals: HashMap<String, u32> = HashMap::new();
    for slot in inventory.backpack_slots.iter().flatten() {
        *totals.entry(slot.type_id.clone()).or_insert(0) += slot.quantity;
    }
    totals
}
