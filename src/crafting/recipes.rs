//! Recipe definitions loaded from `assets/recipes/*.yaml`. Mirrors the
//! `SpellDefinitions` loader pattern in `src/magic/resources.rs`. Recipes
//! are pure data — extending the system to a new recipe is one YAML file.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::discover_yaml_assets;
use crate::player::classes::Class;
use crate::world::object_definitions::OverworldObjectDefinitions;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecipeIngredient {
    pub type_id: String,
    pub count: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AutoLearnSpec {
    /// Class that auto-learns this recipe when reaching `min_level`.
    pub class: Class,
    /// Inclusive level threshold. `0` would mean "from level 1" but the
    /// default keeps recipes off until explicit.
    #[serde(default = "default_min_level")]
    pub min_level: u32,
}

fn default_min_level() -> u32 {
    1
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecipeDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub inputs: Vec<RecipeIngredient>,
    pub outputs: Vec<RecipeIngredient>,
    /// Optional crafting station — when `Some`, the player must be adjacent
    /// to a world object whose `definition_id == station` to craft. `None`
    /// = craftable anywhere.
    #[serde(default)]
    pub station: Option<String>,
    /// Auto-learn rule. Recipes without a rule are only learnable via
    /// scrolls or `<<give_recipe>>`.
    #[serde(default)]
    pub auto_learn: Option<AutoLearnSpec>,
    /// XP awarded on successful craft. `0` means no award.
    #[serde(default)]
    pub xp_award: u64,
}

#[derive(Resource, Default)]
pub struct RecipeDefinitions {
    definitions: HashMap<String, RecipeDefinition>,
    /// Reverse index: object type_id → recipes that require it as station.
    /// Used by the right-click "Craft" menu and the filtered recipe-book
    /// view.
    by_station: HashMap<String, Vec<String>>,
    /// Reverse index for auto-learn lookups, grouped by class. Inner pairs
    /// are `(min_level, recipe_id)`.
    auto_learn_by_class: HashMap<Class, Vec<(u32, String)>>,
}

impl RecipeDefinitions {
    /// Loads every YAML under `assets/recipes/`. Validation of ingredient
    /// `type_id`s is deferred until `validate_against` is called with the
    /// object registry (object defs may not be loaded yet at construction
    /// time, depending on plugin order).
    pub fn load_from_disk() -> Self {
        let mut definitions = HashMap::new();
        for asset in discover_yaml_assets("recipes", "recipe definition") {
            let definition = serde_yaml::from_str::<RecipeDefinition>(&asset.contents)
                .unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse recipe definition {}: {error}",
                        asset.path.display()
                    )
                });
            definitions.insert(asset.id, definition);
        }
        let mut out = Self {
            definitions,
            by_station: HashMap::new(),
            auto_learn_by_class: HashMap::new(),
        };
        out.rebuild_indices();
        out
    }

    fn rebuild_indices(&mut self) {
        self.by_station.clear();
        self.auto_learn_by_class.clear();
        for (id, def) in &self.definitions {
            if let Some(station) = def.station.as_ref() {
                self.by_station
                    .entry(station.clone())
                    .or_default()
                    .push(id.clone());
            }
            if let Some(rule) = def.auto_learn.as_ref() {
                self.auto_learn_by_class
                    .entry(rule.class)
                    .or_default()
                    .push((rule.min_level, id.clone()));
            }
        }
        for list in self.by_station.values_mut() {
            list.sort();
        }
        for list in self.auto_learn_by_class.values_mut() {
            list.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        }
    }

    /// Cross-check every input/output `type_id` against the loaded object
    /// definitions and panic on a typo. Called from `CraftingServerPlugin`
    /// at startup after `OverworldObjectDefinitions` is inserted. Matches
    /// the spell loader's posture: an authoring mistake stops the world.
    pub fn validate_against(&self, objects: &OverworldObjectDefinitions) {
        for (id, def) in &self.definitions {
            for ingredient in def.inputs.iter().chain(def.outputs.iter()) {
                assert!(
                    objects.get(&ingredient.type_id).is_some(),
                    "recipe `{}` references unknown object type_id `{}`",
                    id,
                    ingredient.type_id,
                );
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&RecipeDefinition> {
        self.definitions.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.definitions.keys().map(String::as_str)
    }

    pub fn by_station(&self, station_type_id: &str) -> &[String] {
        self.by_station
            .get(station_type_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Returns `(min_level, recipe_id)` pairs for `class`, sorted by level.
    pub fn auto_learn_for(&self, class: Class) -> &[(u32, String)] {
        self.auto_learn_by_class
            .get(&class)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_recipe_parses() {
        let yaml = r#"
name: Torch
inputs:
  - { type_id: branch, count: 1 }
  - { type_id: oil_flask, count: 1 }
outputs:
  - { type_id: torch, count: 1 }
"#;
        let recipe: RecipeDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(recipe.name, "Torch");
        assert_eq!(recipe.inputs.len(), 2);
        assert!(recipe.station.is_none());
        assert!(recipe.auto_learn.is_none());
        assert_eq!(recipe.xp_award, 0);
    }

    #[test]
    fn full_schema_round_trip() {
        let yaml = r#"
name: Iron Spear
description: A sharpened spear.
inputs:
  - { type_id: branch, count: 1 }
  - { type_id: iron_ingot, count: 2 }
outputs:
  - { type_id: iron_spear, count: 1 }
station: anvil
auto_learn: { class: Fighter, min_level: 4 }
xp_award: 75
"#;
        let recipe: RecipeDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(recipe.station.as_deref(), Some("anvil"));
        let rule = recipe.auto_learn.unwrap();
        assert_eq!(rule.class, Class::Fighter);
        assert_eq!(rule.min_level, 4);
        assert_eq!(recipe.xp_award, 75);
    }

    #[test]
    fn rebuild_indices_groups_by_station_and_class() {
        let mut defs = RecipeDefinitions::default();
        defs.definitions.insert(
            "a".to_owned(),
            RecipeDefinition {
                name: "A".to_owned(),
                description: String::new(),
                inputs: vec![],
                outputs: vec![],
                station: Some("anvil".to_owned()),
                auto_learn: Some(AutoLearnSpec {
                    class: Class::Fighter,
                    min_level: 1,
                }),
                xp_award: 0,
            },
        );
        defs.definitions.insert(
            "b".to_owned(),
            RecipeDefinition {
                name: "B".to_owned(),
                description: String::new(),
                inputs: vec![],
                outputs: vec![],
                station: Some("anvil".to_owned()),
                auto_learn: None,
                xp_award: 0,
            },
        );
        defs.rebuild_indices();
        let mut anvil = defs.by_station("anvil").to_vec();
        anvil.sort();
        assert_eq!(anvil, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(defs.auto_learn_for(Class::Fighter).len(), 1);
        assert!(defs.auto_learn_for(Class::Wizard).is_empty());
    }

    #[test]
    fn all_authored_recipes_parse_from_disk() {
        let defs = RecipeDefinitions::load_from_disk();
        let ids: Vec<&str> = defs.ids().collect();
        assert!(
            ids.contains(&"mushroom_brew") && ids.contains(&"bolt_from_arrows"),
            "expected baseline recipes; got {ids:?}"
        );
    }

    #[test]
    fn authored_recipes_validate_against_object_defs() {
        let recipes = RecipeDefinitions::load_from_disk();
        let objects = OverworldObjectDefinitions::load_from_disk();
        // Should not panic — every authored recipe must reference real items.
        recipes.validate_against(&objects);
    }
}
