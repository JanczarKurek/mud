//! Crafting: per-character stash, recipes, crafting commands, learning.
//!
//! The stash is the foundation — a generic JSON key/value store on every
//! player that other subsystems (recipes, quests, future features) can write
//! to. Persisted via `PlayerStateDump`.

pub mod learning;
pub mod recipes;
pub mod stash;
pub mod systems;

use bevy::prelude::*;

pub use recipes::{AutoLearnSpec, RecipeDefinition, RecipeDefinitions, RecipeIngredient};
pub use stash::{CharacterStash, LEARNED_RECIPES_KEY};

use crate::app::state::simulation_active;
use crate::game::CommandIntercept;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Shared system set for crafting work. `Process` runs inside
/// `game::CommandIntercept` so stash/recipe/craft commands are drained
/// before `process_game_commands` sees them.
#[derive(SystemSet, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CraftingSystemSet {
    Process,
}

/// Server-side crafting systems: stash mutation, learn/craft commands, etc.
/// Registered alongside `MagicPlugin` in all server-running runtime modes.
pub struct CraftingServerPlugin;

impl Plugin for CraftingServerPlugin {
    fn build(&self, app: &mut App) {
        let recipes = RecipeDefinitions::load_from_disk();
        app.insert_resource(recipes)
            .add_systems(Startup, validate_recipes_against_objects)
            .configure_sets(Update, CraftingSystemSet::Process.in_set(CommandIntercept))
            .add_systems(
                Update,
                (
                    learning::auto_learn_for_changed_progression,
                    systems::process_stash_commands,
                    learning::process_learn_recipe_commands,
                    systems::process_craft_commands,
                )
                    .chain()
                    .in_set(CraftingSystemSet::Process)
                    .run_if(simulation_active),
            );
    }
}

/// Client-side crafting presentation: recipe-book UI, learn toasts.
pub struct CraftingClientPlugin;

impl Plugin for CraftingClientPlugin {
    fn build(&self, app: &mut App) {
        // Inserted on the client too so the recipe-book UI can render
        // recipe metadata locally without server round-trips.
        app.insert_resource(RecipeDefinitions::load_from_disk());
        crate::ui::recipe_book::register(app);
    }
}

/// Startup cross-check: panic on any recipe that references a missing
/// object type_id. Runs after `OverworldObjectDefinitions` is inserted so
/// the registry is populated.
fn validate_recipes_against_objects(
    recipes: Res<RecipeDefinitions>,
    objects: Res<OverworldObjectDefinitions>,
) {
    recipes.validate_against(&objects);
}
