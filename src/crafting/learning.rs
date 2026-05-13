//! Recipe-learning paths. Three sources feed into the single
//! [`learn_recipe`] helper:
//! 1. `<<give_recipe "id">>` Yarn directive (registered in dialog).
//! 2. Items with `learns_recipe: <id>` (handled in `handle_use_item`).
//! 3. Auto-learn on class/level match (driven by Added<Player> and
//!    Changed<Experience> in Step 7).

use bevy::prelude::*;

use crate::crafting::recipes::RecipeDefinitions;
use crate::crafting::stash::CharacterStash;
use crate::game::commands::GameCommand;
use crate::game::resources::{
    ChatLogState, GameEvent, GameUiEvent, PendingGameCommands, PendingGameEvents,
    PendingGameUiEvents,
};
use crate::player::classes::Class;
use crate::player::components::{Player, PlayerId, PlayerIdentity};
use crate::player::progression::Experience;

/// Drains `GameCommand::LearnRecipe`. Idempotent — re-granting a known
/// recipe emits no events. On first-time grants, pushes
/// `GameEvent::RecipeLearned` (replicated to the player's client),
/// `GameUiEvent::RecipeLearnedToast` (drives the popup), and a narrator
/// line on the player's chat log.
pub fn process_learn_recipe_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut pending_events: ResMut<PendingGameEvents>,
    mut pending_ui_events: ResMut<PendingGameUiEvents>,
    recipe_defs: Res<RecipeDefinitions>,
    mut players: Query<(&PlayerIdentity, &mut CharacterStash, &mut ChatLogState), With<Player>>,
) {
    if pending_commands.commands.is_empty() {
        return;
    }
    let drained: Vec<_> = std::mem::take(&mut pending_commands.commands);
    let mut remaining = Vec::with_capacity(drained.len());

    for queued in drained {
        let GameCommand::LearnRecipe { ref recipe_id } = queued.command else {
            remaining.push(queued);
            continue;
        };

        let acting = queued
            .player_id
            .or_else(|| players.iter().next().map(|(identity, _, _)| identity.id));
        let Some(PlayerId(target_id)) = acting else {
            continue;
        };

        let Some(recipe) = recipe_defs.get(recipe_id) else {
            bevy::log::warn!("LearnRecipe: unknown recipe `{recipe_id}` (dropped)");
            continue;
        };

        for (identity, mut stash, mut chat_log) in players.iter_mut() {
            if identity.id.0 != target_id {
                continue;
            }
            if !stash.add_learned_recipe(recipe_id) {
                // Already known — silent.
                break;
            }
            chat_log
                .lines
                .push(format!("You learn the recipe for {}.", recipe.name));
            pending_events.events.push(GameEvent::RecipeLearned {
                recipe_id: recipe_id.clone(),
            });
            pending_ui_events.push(
                identity.id,
                GameUiEvent::RecipeLearnedToast {
                    recipe_id: recipe_id.clone(),
                    recipe_name: recipe.name.clone(),
                },
            );
            break;
        }
    }

    pending_commands.commands = remaining;
}

/// Queue `LearnRecipe` for every auto-learn recipe whose class matches
/// the player's and whose `min_level <= player.level`. Idempotent —
/// `process_learn_recipe_commands` silently no-ops on re-grants. Runs on
/// `Added<Player>` (fresh spawn + login + respawn) and is also re-fired
/// whenever a player's `Experience` changes so a fresh level-up grants
/// the next tier of recipes without waiting for a full relog.
pub fn auto_learn_for_changed_progression(
    recipe_defs: Res<RecipeDefinitions>,
    mut pending_commands: ResMut<PendingGameCommands>,
    players: Query<
        (&PlayerIdentity, Option<&Class>, Option<&Experience>),
        Or<(Added<Player>, Changed<Experience>)>,
    >,
) {
    for (identity, class, experience) in &players {
        let class = class.copied().unwrap_or_default();
        let level = experience.map(|e| e.level).unwrap_or(1);
        for (min_level, recipe_id) in recipe_defs.auto_learn_for(class) {
            if *min_level > level {
                // Sorted by level — once we pass the threshold we're done.
                break;
            }
            pending_commands.push_for_player(
                identity.id,
                GameCommand::LearnRecipe {
                    recipe_id: recipe_id.clone(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a minimal test App. Loads the authored
    /// `RecipeDefinitions` from disk (no in-memory construction is exposed
    /// publicly, and the disk loader is fast).
    fn build_test_app() -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(RecipeDefinitions::load_from_disk());
        app.insert_resource(PendingGameCommands::default());
        app.insert_resource(PendingGameEvents::default());
        app.insert_resource(PendingGameUiEvents::default());
        app.add_systems(Update, process_learn_recipe_commands);
        let entity = app
            .world_mut()
            .spawn((
                Player,
                PlayerIdentity::new(PlayerId(1)),
                CharacterStash::default(),
                ChatLogState::default(),
            ))
            .id();
        (app, entity)
    }

    #[test]
    fn learn_recipe_emits_event_first_time_only() {
        let (mut app, entity) = build_test_app();

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::LearnRecipe {
                    recipe_id: "mushroom_brew".to_owned(),
                },
            );
        app.update();

        let stash = app.world().entity(entity).get::<CharacterStash>().unwrap();
        assert!(stash.learned_recipes().contains("mushroom_brew"));
        let events = &app.world().resource::<PendingGameEvents>().events;
        assert_eq!(events.len(), 1, "first learn emits one delta event");

        app.world_mut()
            .resource_mut::<PendingGameEvents>()
            .events
            .clear();
        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::LearnRecipe {
                    recipe_id: "mushroom_brew".to_owned(),
                },
            );
        app.update();
        assert!(
            app.world()
                .resource::<PendingGameEvents>()
                .events
                .is_empty(),
            "second learn is idempotent"
        );
    }

    #[test]
    fn learn_recipe_unknown_id_drops() {
        let (mut app, entity) = build_test_app();

        app.world_mut()
            .resource_mut::<PendingGameCommands>()
            .push_for_player(
                PlayerId(1),
                GameCommand::LearnRecipe {
                    recipe_id: "bogus_typo".to_owned(),
                },
            );
        app.update();

        let stash = app.world().entity(entity).get::<CharacterStash>().unwrap();
        assert!(stash.learned_recipes().is_empty());
    }
}
