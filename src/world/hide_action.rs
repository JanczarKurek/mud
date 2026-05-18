//! Player-driven `Hide` on a nearby world object that has `can_hide:` in its
//! definition. Drained from `PendingGameCommands` in `CommandIntercept`,
//! alongside `process_interact_commands`.
//!
//! On a successful Thievery check (DC 10 + the item's `sneakiness` as
//! situational bonus) the object gains the `Hidden` component with
//! `dc = check_total / 2`, and the placer is seeded into `detected_by` so
//! they keep seeing it. The projection's per-peer filter
//! (`game/projection.rs:521`) does the rest — other players' next tick
//! receives a `WorldObjectRemoved` event.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::game::commands::GameCommand;
use crate::game::helpers::is_near_player;
use crate::game::resources::{PendingGameCommands, QueuedGameCommand};
use crate::player::components::{BaseStats, ChatLog, Player, PlayerIdentity};
use crate::player::skills::{skill_check, Skill, SkillSheet};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::hidden::Hidden;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Baseline DC the Thievery check must beat for a Hide attempt to take effect.
/// Below this, the action fails and a narrator line is emitted.
pub const HIDE_THRESHOLD_DC: i32 = 10;

/// Drains `GameCommand::HideObject` entries from `PendingGameCommands` and
/// applies them. Other commands are pushed back onto the queue untouched.
pub fn process_hide_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    definitions: Res<OverworldObjectDefinitions>,
    mut commands: Commands,
    object_query: Query<
        (
            Entity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
            Option<&Hidden>,
        ),
        Without<Player>,
    >,
    mut player_query: Query<
        (
            &PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &BaseStats,
            &SkillSheet,
            &mut ChatLog,
        ),
        With<Player>,
    >,
) {
    let drained: Vec<QueuedGameCommand> = pending_commands.commands.drain(..).collect();
    let mut remaining = Vec::with_capacity(drained.len());

    for queued in drained {
        let object_id = match queued.command {
            GameCommand::HideObject { object_id } => object_id,
            other => {
                remaining.push(QueuedGameCommand {
                    player_id: queued.player_id,
                    command: other,
                });
                continue;
            }
        };

        let Some((identity, player_space, player_tile, base_stats, skill_sheet, mut chat_log)) =
            (match queued.player_id {
                Some(id) => player_query.iter_mut().find(|row| row.0.id == id),
                None => player_query.iter_mut().next(),
            })
        else {
            continue;
        };
        let placer_id = identity.id;

        let Some((entity, _, _, object, already_hidden)) =
            object_query.iter().find(|(_, resident, tile, object, _)| {
                resident.space_id == player_space.space_id
                    && object.object_id == object_id
                    && is_near_player(player_tile, tile)
            })
        else {
            bevy::log::debug!(
                "HideObject {object_id} ignored: not adjacent, missing, or different space"
            );
            continue;
        };

        let Some(definition) = definitions.get(&object.definition_id) else {
            bevy::log::debug!(
                "HideObject {object_id}: missing definition '{}'",
                object.definition_id
            );
            continue;
        };

        let Some(can_hide) = definition.can_hide else {
            chat_log.push_narrator(format!(
                "You can't hide the {}.",
                definition.name.to_lowercase()
            ));
            continue;
        };

        if already_hidden.is_some() {
            chat_log.push_narrator(format!(
                "The {} is already hidden.",
                definition.name.to_lowercase()
            ));
            continue;
        }

        if skill_sheet.rank(Skill::Thievery) == 0 {
            chat_log.push_narrator("You don't know the first thing about hiding.");
            continue;
        }

        let result = skill_check(
            skill_sheet,
            &base_stats.attributes,
            Skill::Thievery,
            HIDE_THRESHOLD_DC,
            can_hide.sneakiness,
        );
        let name = definition.name.to_lowercase();
        if !result.success {
            chat_log.push_narrator(format!(
                "You fail to hide the {name}. (Thievery {} vs DC {HIDE_THRESHOLD_DC})",
                result.total
            ));
            continue;
        }

        let dc = (result.total / 2).max(0) as u32;
        let mut detected_by = HashSet::new();
        detected_by.insert(placer_id);
        commands.entity(entity).insert(Hidden {
            dc,
            detected_by,
            next_check_at: HashMap::new(),
        });

        chat_log.push_narrator(format!(
            "You hide the {name}. (Thievery {}, DC to spot {dc})",
            result.total
        ));
    }

    pending_commands.commands = remaining;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::resources::QueuedGameCommand;
    use crate::player::components::{AttributeSet, PlayerId};
    use crate::world::components::SpaceId;
    use crate::world::object_definitions::{CanHideDef, OverworldObjectDefinition};
    use std::collections::HashMap as StdHashMap;

    const FIXTURE_YAML: &str = r#"
name: Trinket
description: ""
colliding: false
movable: true
storable: true
render:
  z_index: 0.0
  debug_color: [0, 0, 0]
  debug_size: 1.0
"#;

    fn fixture_definitions(with_can_hide: bool) -> OverworldObjectDefinitions {
        let mut def: OverworldObjectDefinition =
            serde_yaml::from_str(FIXTURE_YAML).expect("test fixture parses");
        if with_can_hide {
            def.can_hide = Some(CanHideDef { sneakiness: 0 });
        }
        let mut map = StdHashMap::new();
        map.insert("trinket".to_string(), def);
        OverworldObjectDefinitions::new_for_test(map)
    }

    fn build_app(defs: OverworldObjectDefinitions) -> App {
        let mut app = App::new();
        app.insert_resource(defs);
        app.insert_resource(PendingGameCommands::default());
        app.add_systems(Update, process_hide_commands);
        app
    }

    fn spawn_player_and_object(
        app: &mut App,
        space: SpaceId,
        player_tile: TilePosition,
        object_tile: TilePosition,
        thievery_ranks: u8,
        already_hidden: bool,
    ) -> (PlayerId, u64, Entity) {
        let player_id = PlayerId(7);
        let mut sheet = SkillSheet::default();
        sheet.set_rank(Skill::Thievery, thievery_ranks);
        app.world_mut().spawn((
            Player,
            PlayerIdentity::new(player_id),
            SpaceResident { space_id: space },
            player_tile,
            BaseStats {
                attributes: AttributeSet::new(10, 14, 10, 10, 10, 10),
                ..Default::default()
            },
            sheet,
            ChatLog::default(),
        ));
        let object_id = 42u64;
        let mut entity_commands = app.world_mut().spawn((
            OverworldObject {
                object_id,
                definition_id: "trinket".to_string(),
            },
            SpaceResident { space_id: space },
            object_tile,
        ));
        if already_hidden {
            entity_commands.insert(Hidden::new(5));
        }
        let entity = entity_commands.id();
        (player_id, object_id, entity)
    }

    fn queue_hide(app: &mut App, player_id: PlayerId, object_id: u64) {
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .commands
            .push(QueuedGameCommand {
                player_id: Some(player_id),
                command: GameCommand::HideObject { object_id },
            });
    }

    #[test]
    fn hide_on_object_without_can_hide_does_not_insert_hidden() {
        let mut app = build_app(fixture_definitions(false));
        let (player_id, object_id, entity) = spawn_player_and_object(
            &mut app,
            SpaceId(0),
            TilePosition::ground(0, 0),
            TilePosition::ground(0, 0),
            5,
            false,
        );
        queue_hide(&mut app, player_id, object_id);
        app.update();
        assert!(app.world().entity(entity).get::<Hidden>().is_none());
    }

    #[test]
    fn hide_with_zero_thievery_ranks_fails() {
        let mut app = build_app(fixture_definitions(true));
        let (player_id, object_id, entity) = spawn_player_and_object(
            &mut app,
            SpaceId(0),
            TilePosition::ground(0, 0),
            TilePosition::ground(0, 0),
            0,
            false,
        );
        queue_hide(&mut app, player_id, object_id);
        app.update();
        assert!(app.world().entity(entity).get::<Hidden>().is_none());
    }

    #[test]
    fn hide_on_already_hidden_object_is_noop() {
        let mut app = build_app(fixture_definitions(true));
        let (player_id, object_id, entity) = spawn_player_and_object(
            &mut app,
            SpaceId(0),
            TilePosition::ground(0, 0),
            TilePosition::ground(0, 0),
            5,
            true,
        );
        let original_dc = app.world().entity(entity).get::<Hidden>().unwrap().dc;
        queue_hide(&mut app, player_id, object_id);
        app.update();
        let hidden = app.world().entity(entity).get::<Hidden>().unwrap();
        assert_eq!(hidden.dc, original_dc);
        assert!(hidden.detected_by.is_empty());
    }

    #[test]
    fn non_adjacent_player_cannot_hide() {
        let mut app = build_app(fixture_definitions(true));
        let (player_id, object_id, entity) = spawn_player_and_object(
            &mut app,
            SpaceId(0),
            TilePosition::ground(0, 0),
            TilePosition::ground(5, 5),
            5,
            false,
        );
        queue_hide(&mut app, player_id, object_id);
        app.update();
        assert!(app.world().entity(entity).get::<Hidden>().is_none());
    }
}
