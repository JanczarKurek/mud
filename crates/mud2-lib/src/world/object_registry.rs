use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::magic::resources::SpellDefinitions;
use crate::world::direction::Direction;
use crate::world::map_layout::{
    MapBehavior, ObjectProperties, ResolvedObject, RoutineInstanceDef, SpaceDefinitions,
};
use crate::world::object_definitions::{
    number_to_customary, number_to_written, OverworldObjectDefinitions,
};

#[derive(Resource, Default)]
pub struct ObjectRegistry {
    type_ids: HashMap<u64, String>,
    properties: HashMap<u64, ObjectProperties>,
    behaviors: HashMap<u64, MapBehavior>,
    authored: HashMap<u64, AuthoredMeta>,
    next_runtime_id: u64,
}

/// Authored-only fields that have no authoritative runtime representation but
/// must survive an editor save round-trip.
///
/// `facing` and `routine` do get baked into components at spawn, but reading
/// them back from the ECS would be wrong: an object with no authored `facing:`
/// still gets a `Facing` component from its definition's `default_facing`, so
/// the ECS cannot distinguish "authored north" from "defaulted north" and a
/// save would sprout a `facing:` on every object in the map. `authored_id` has
/// no runtime representation at all. Absent for objects created at runtime.
#[derive(Clone, Debug, Default)]
pub struct AuthoredMeta {
    pub authored_id: Option<String>,
    pub facing: Option<Direction>,
    pub routine: Option<RoutineInstanceDef>,
    pub quantity: Option<u32>,
}

impl AuthoredMeta {
    pub fn from_resolved(object: &ResolvedObject) -> Self {
        Self {
            authored_id: object.authored_id.clone(),
            facing: object.facing,
            routine: object.routine.clone(),
            quantity: object.quantity,
        }
    }

    fn is_empty(&self) -> bool {
        self.authored_id.is_none()
            && self.facing.is_none()
            && self.routine.is_none()
            && self.quantity.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectRegistrySnapshotEntry {
    pub object_id: u64,
    pub type_id: String,
    pub properties: ObjectProperties,
}

impl ObjectRegistry {
    pub fn from_space_definitions(space_definitions: &SpaceDefinitions) -> Self {
        let mut type_ids = HashMap::new();
        let mut properties = HashMap::new();
        let mut max_id = 0;

        let mut behaviors = HashMap::new();
        let mut authored = HashMap::new();

        for definition in space_definitions.iter() {
            for object in &definition.resolved_objects {
                let previous = type_ids.insert(object.id, object.type_id.clone());
                assert!(
                    previous.is_none(),
                    "Duplicate authored object id {} across spaces",
                    object.id
                );
                properties.insert(object.id, object.properties.clone());
                if let Some(behavior) = &object.behavior {
                    behaviors.insert(object.id, *behavior);
                }
                let meta = AuthoredMeta::from_resolved(object);
                if !meta.is_empty() {
                    authored.insert(object.id, meta);
                }
                max_id = max_id.max(object.id);
            }
        }

        Self {
            type_ids,
            properties,
            behaviors,
            authored,
            next_runtime_id: max_id + 1,
        }
    }

    pub fn from_snapshot(entries: Vec<ObjectRegistrySnapshotEntry>, next_runtime_id: u64) -> Self {
        let mut type_ids = HashMap::new();
        let mut properties = HashMap::new();

        for entry in entries {
            type_ids.insert(entry.object_id, entry.type_id);
            properties.insert(entry.object_id, entry.properties);
        }

        Self {
            type_ids,
            properties,
            behaviors: HashMap::new(),
            // World snapshots carry no authored metadata; the editor's source
            // of truth for it is the map YAML, reloaded on file-open.
            authored: HashMap::new(),
            next_runtime_id,
        }
    }

    pub fn type_id(&self, object_id: u64) -> Option<&str> {
        self.type_ids.get(&object_id).map(String::as_str)
    }

    pub fn allocate_runtime_id(&mut self, type_id: impl Into<String>) -> u64 {
        self.allocate_runtime_id_with_properties(type_id, ObjectProperties::new())
    }

    pub fn allocate_runtime_id_with_properties(
        &mut self,
        type_id: impl Into<String>,
        properties: ObjectProperties,
    ) -> u64 {
        let object_id = self.next_runtime_id;
        self.next_runtime_id += 1;
        self.type_ids.insert(object_id, type_id.into());
        self.properties.insert(object_id, properties);
        object_id
    }

    pub fn properties(&self, object_id: u64) -> Option<&ObjectProperties> {
        self.properties.get(&object_id)
    }

    /// Mutable access to a registered object's properties bag. Used by the
    /// interaction system to mirror an `ObjectState` change into
    /// `properties["state"]` so the existing persistence path captures it.
    pub fn properties_mut(&mut self, object_id: u64) -> Option<&mut ObjectProperties> {
        self.properties.get_mut(&object_id)
    }

    pub fn behavior(&self, object_id: u64) -> Option<&MapBehavior> {
        self.behaviors.get(&object_id)
    }

    /// Authored-YAML metadata for an object (symbolic id, facing, routine,
    /// quantity). `None` for anything created at runtime. Read by the editor's
    /// serializer so a save round-trips what the author wrote.
    pub fn authored_meta(&self, object_id: u64) -> Option<&AuthoredMeta> {
        self.authored.get(&object_id)
    }

    /// Attach (or, when every field is empty, clear) an object's authored
    /// metadata. Keeping empty entries out of the map means `authored_meta`
    /// answers "was anything authored here?" without a per-field check.
    pub fn set_authored_meta(&mut self, object_id: u64, meta: AuthoredMeta) {
        if meta.is_empty() {
            self.authored.remove(&object_id);
        } else {
            self.authored.insert(object_id, meta);
        }
    }

    /// Replace (or remove) the behavior on a registered object. Used by the
    /// editor's per-NPC behavior panel; `None` clears any existing behavior.
    pub fn set_behavior(&mut self, object_id: u64, behavior: Option<MapBehavior>) {
        match behavior {
            Some(b) => {
                self.behaviors.insert(object_id, b);
            }
            None => {
                self.behaviors.remove(&object_id);
            }
        }
    }

    pub fn set_properties(&mut self, object_id: u64, properties: ObjectProperties) {
        self.properties.insert(object_id, properties);
    }

    pub fn next_runtime_id(&self) -> u64 {
        self.next_runtime_id
    }

    /// Re-register an object id that was allocated in a previous session (e.g.
    /// loaded from an account DB row) so future `allocate_runtime_id` calls do
    /// not collide with it. Preserves existing properties when the type is
    /// unchanged; clears them (and behavior) when the type flips, since those
    /// fields belonged to whatever object previously owned this id and are
    /// stale by definition once the slot is reassigned.
    pub fn register_existing(&mut self, object_id: u64, type_id: impl Into<String>) {
        let new_type = type_id.into();
        let type_changed = match self.type_ids.insert(object_id, new_type.clone()) {
            Some(prev) => prev != new_type,
            None => false,
        };
        if type_changed {
            self.properties.insert(object_id, ObjectProperties::new());
            self.behaviors.remove(&object_id);
            self.authored.remove(&object_id);
        } else {
            self.properties.entry(object_id).or_default();
        }
        if self.next_runtime_id <= object_id {
            self.next_runtime_id = object_id + 1;
        }
    }

    /// Replace any existing registry entry for `object_id` with the given
    /// type / properties / behavior. Use when the registry needs to be
    /// brought back in line with an authoritative source (e.g. file-open
    /// rebuilding a space from its YAML); plain `register_existing` is fine
    /// when properties should be left alone, but reset paths need the full
    /// overwrite so they don't inherit state from a previous owner of the
    /// id slot.
    pub fn replace_existing(
        &mut self,
        object_id: u64,
        type_id: impl Into<String>,
        properties: ObjectProperties,
        behavior: Option<MapBehavior>,
    ) {
        self.type_ids.insert(object_id, type_id.into());
        self.properties.insert(object_id, properties);
        match behavior {
            Some(b) => {
                self.behaviors.insert(object_id, b);
            }
            None => {
                self.behaviors.remove(&object_id);
            }
        }
        // Reset path: drop any authored metadata left by the id slot's previous
        // owner. `sync_resolved_objects` re-attaches the correct one right after.
        self.authored.remove(&object_id);
        if self.next_runtime_id <= object_id {
            self.next_runtime_id = object_id + 1;
        }
    }

    /// Overwrite the registry entries for a freshly-resolved space so its
    /// runtime ids, types, properties, and behaviors are authoritative, and
    /// advance `next_runtime_id` past them. Mirrors the loop in
    /// `reset_space_contents_from_def` and the editor's GenerateDungeon path;
    /// call it after `SpaceDefinitions::load_single_from_disk` (which re-resolves
    /// a space into a fresh id range but does not touch the registry) so the
    /// re-baked ids can't later be re-handed-out by `allocate_runtime_id` and
    /// collide with a live entity of a different type.
    pub fn sync_resolved_objects(&mut self, objects: &[ResolvedObject]) {
        for object in objects {
            self.replace_existing(
                object.id,
                object.type_id.clone(),
                object.properties.clone(),
                object.behavior,
            );
            self.set_authored_meta(object.id, AuthoredMeta::from_resolved(object));
        }
    }

    pub fn snapshot_entries(&self) -> Vec<ObjectRegistrySnapshotEntry> {
        let mut entries = self
            .type_ids
            .iter()
            .map(|(object_id, type_id)| ObjectRegistrySnapshotEntry {
                object_id: *object_id,
                type_id: type_id.clone(),
                properties: self.properties.get(object_id).cloned().unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.object_id);
        entries
    }

    pub fn display_name(
        &self,
        object_id: u64,
        definitions: &OverworldObjectDefinitions,
        spell_definitions: &SpellDefinitions,
    ) -> Option<String> {
        let type_id = self.type_id(object_id)?;
        let properties = self.properties(object_id);
        Self::display_name_for_type(type_id, properties, definitions, spell_definitions)
    }

    /// Resolve a display name from a raw `(type_id, properties)` pair, without
    /// requiring the object to be registered with a runtime id. Use this for
    /// inventory stacks, equipment slots, and other "in-the-bag" descriptors
    /// where the item has no live `OverworldObject`.
    pub fn display_name_for_type(
        type_id: &str,
        properties: Option<&ObjectProperties>,
        definitions: &OverworldObjectDefinitions,
        spell_definitions: &SpellDefinitions,
    ) -> Option<String> {
        let definition = definitions.get(type_id)?;
        Some(render_template(
            properties,
            &definition.name,
            spell_definitions,
            1,
        ))
    }

    pub fn description_with_count(
        &self,
        object_id: u64,
        count: u32,
        definitions: &OverworldObjectDefinitions,
        spell_definitions: &SpellDefinitions,
    ) -> Option<String> {
        let type_id = self.type_id(object_id)?;
        let properties = self.properties(object_id);
        Self::description_with_count_for_type(
            type_id,
            properties,
            count,
            definitions,
            spell_definitions,
        )
    }

    pub fn description_with_count_for_type(
        type_id: &str,
        properties: Option<&ObjectProperties>,
        count: u32,
        definitions: &OverworldObjectDefinitions,
        spell_definitions: &SpellDefinitions,
    ) -> Option<String> {
        let definition = definitions.get(type_id)?;
        let template = definition.description_for_count(count);
        Some(render_template(
            properties,
            template,
            spell_definitions,
            count,
        ))
    }

    pub fn resolved_spell_id(
        &self,
        object_id: u64,
        definitions: &OverworldObjectDefinitions,
        spell_definitions: &SpellDefinitions,
    ) -> Option<String> {
        let type_id = self.type_id(object_id)?;
        let properties = self.properties(object_id);
        Self::resolved_spell_id_for_type(type_id, properties, definitions, spell_definitions)
    }

    pub fn resolved_spell_id_for_type(
        type_id: &str,
        properties: Option<&ObjectProperties>,
        definitions: &OverworldObjectDefinitions,
        spell_definitions: &SpellDefinitions,
    ) -> Option<String> {
        let definition = definitions.get(type_id)?;
        definition
            .spell_id
            .as_ref()
            .map(|template| render_template(properties, template, spell_definitions, 1))
    }
}

fn render_template(
    properties: Option<&ObjectProperties>,
    template: &str,
    spell_definitions: &SpellDefinitions,
    count: u32,
) -> String {
    let mut rendered = String::new();
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        rendered.push_str(&rest[..start]);
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('}') else {
            rendered.push_str(&rest[start..]);
            return rendered;
        };

        let expression = &after_start[..end];
        let resolved = resolve_count_expression(expression, count).or_else(|| {
            properties.and_then(|p| resolve_template_expression(expression, p, spell_definitions))
        });
        rendered.push_str(&resolved.unwrap_or_else(|| format!("{{{expression}}}")));
        rest = &after_start[end + 1..];
    }

    rendered.push_str(rest);
    rendered
}

fn resolve_count_expression(expression: &str, count: u32) -> Option<String> {
    match expression {
        "count" => Some(count.to_string()),
        "count_written" => Some(number_to_written(count)),
        "count_customary" => Some(
            number_to_customary(count)
                .map(str::to_owned)
                .unwrap_or_else(|| number_to_written(count)),
        ),
        _ => None,
    }
}

fn resolve_template_expression(
    expression: &str,
    properties: &ObjectProperties,
    spell_definitions: &SpellDefinitions,
) -> Option<String> {
    // `{properties.foo|fallback text}` resolves to the property value, or
    // `fallback text` when the property is missing/empty. Used by book/
    // tombstone metadata so untitled instances still get a readable name.
    let (head, fallback) = match expression.split_once('|') {
        Some((head, fb)) => (head, Some(fb.to_owned())),
        None => (expression, None),
    };
    let property = head.strip_prefix("properties.")?;
    if let Some(property_name) = property.strip_suffix(".name") {
        if let Some(property_value) = properties.get(property_name) {
            if let Some(spell) = spell_definitions.get(property_value) {
                return Some(spell.name.clone());
            }
        }
        return fallback;
    }

    let direct = properties.get(property).cloned().filter(|s| !s.is_empty());
    direct.or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::map_layout::TileRectangle;

    fn props_with(key: &str, value: &str) -> ObjectProperties {
        let mut p = ObjectProperties::new();
        p.insert(key.to_owned(), value.to_owned());
        p
    }

    /// Regression: `register_existing` used to leave stale properties on the
    /// slot when the type flipped. The editor's file-open path could then
    /// reassign id N from (e.g.) `wooden_door` to `water`, and the next save
    /// would silently attach the door's `state: locked` onto the new water
    /// entity. The fix clears properties + behavior whenever the type
    /// actually changes.
    #[test]
    fn register_existing_clears_stale_properties_on_type_change() {
        let mut registry = ObjectRegistry::default();
        registry.replace_existing(
            42,
            "wooden_door",
            props_with("state", "locked"),
            Some(MapBehavior::Roam {
                bounds: TileRectangle {
                    min_x: 0,
                    min_y: 0,
                    max_x: 1,
                    max_y: 1,
                },
            }),
        );
        // Same type → preserve.
        registry.register_existing(42, "wooden_door");
        assert_eq!(registry.type_id(42), Some("wooden_door"));
        assert_eq!(
            registry.properties(42).and_then(|p| p.get("state")),
            Some(&"locked".to_owned())
        );
        assert!(registry.behavior(42).is_some());

        // Type flip → clear.
        registry.register_existing(42, "water");
        assert_eq!(registry.type_id(42), Some("water"));
        assert!(registry
            .properties(42)
            .map(|p| p.is_empty())
            .unwrap_or(true));
        assert!(registry.behavior(42).is_none());
    }

    /// `sync_resolved_objects` must bind every resolved id to its type and
    /// advance `next_runtime_id` past the highest one, so a following
    /// `allocate_runtime_id` can't re-hand-out a synced id.
    #[test]
    fn sync_resolved_objects_binds_types_and_advances_counter() {
        use crate::world::map_layout::ResolvedObject;

        fn resolved(id: u64, type_id: &str) -> ResolvedObject {
            ResolvedObject {
                id,
                authored_id: None,
                type_id: type_id.to_owned(),
                properties: ObjectProperties::new(),
                placement: None,
                contents: Vec::new(),
                quantity: None,
                behavior: None,
                facing: None,
                routine: None,
            }
        }

        let mut registry = ObjectRegistry::default();
        registry.sync_resolved_objects(&[resolved(600, "crate"), resolved(605, "wall_n")]);

        assert_eq!(registry.type_id(600), Some("crate"));
        assert_eq!(registry.type_id(605), Some("wall_n"));
        assert_eq!(
            registry.next_runtime_id(),
            606,
            "counter advanced past the highest synced id"
        );
        // The next allocation must not collide with a synced id.
        let fresh = registry.allocate_runtime_id("apple");
        assert!(fresh >= 606);
    }

    #[test]
    fn template_fallback_uses_default_when_property_missing() {
        let props = ObjectProperties::new();
        let spells = SpellDefinitions::default();
        // Missing property → fallback string substitutes.
        assert_eq!(
            render_template(Some(&props), "{properties.title|Untitled}", &spells, 1),
            "Untitled"
        );
        // Present property wins over fallback.
        let p = props_with("title", "Spellbook");
        assert_eq!(
            render_template(Some(&p), "{properties.title|Untitled}", &spells, 1),
            "Spellbook"
        );
        // Empty string is treated as "missing" so the fallback kicks in —
        // matches how the book/tombstone description templates collapse
        // gracefully on un-engraved instances.
        let p = props_with("inscription_line", "");
        assert_eq!(
            render_template(
                Some(&p),
                "A sword. {properties.inscription_line|}",
                &spells,
                1
            ),
            "A sword. "
        );
    }

    /// `replace_existing` always overwrites — that's its point. It's the
    /// hammer used by reset paths that need the registry slot to match an
    /// authoritative source (the freshly-parsed YAML) regardless of what
    /// the slot used to hold.
    #[test]
    fn replace_existing_overwrites_everything() {
        let mut registry = ObjectRegistry::default();
        registry.replace_existing(7, "scroll", props_with("text", "old"), None);
        registry.replace_existing(7, "water", ObjectProperties::new(), None);
        assert_eq!(registry.type_id(7), Some("water"));
        assert!(registry.properties(7).unwrap().is_empty());
        // next_runtime_id bumps so future allocate calls don't collide.
        assert!(registry.next_runtime_id() > 7);
    }
}
