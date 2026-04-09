use std::collections::HashMap;

use bevy::prelude::*;

use crate::magic::resources::SpellDefinitions;
use crate::world::map_layout::MapLayout;
use crate::world::map_layout::ObjectProperties;
use crate::world::object_definitions::OverworldObjectDefinitions;

#[derive(Resource, Default)]
pub struct ObjectRegistry {
    type_ids: HashMap<u64, String>,
    properties: HashMap<u64, ObjectProperties>,
    next_runtime_id: u64,
}

impl ObjectRegistry {
    pub fn from_map_layout(map_layout: &MapLayout) -> Self {
        let mut type_ids = HashMap::new();
        let mut properties = HashMap::new();
        let mut max_id = 0;

        for object in &map_layout.resolved_objects {
            type_ids.insert(object.id, object.type_id.clone());
            properties.insert(object.id, object.properties.clone());
            max_id = max_id.max(object.id);
        }

        Self {
            type_ids,
            properties,
            next_runtime_id: max_id + 1,
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

    pub fn display_name(
        &self,
        object_id: u64,
        definitions: &OverworldObjectDefinitions,
        spell_definitions: &SpellDefinitions,
    ) -> Option<String> {
        let type_id = self.type_id(object_id)?;
        let definition = definitions.get(type_id)?;
        Some(self.render_template(object_id, &definition.name, spell_definitions))
    }

    pub fn description(
        &self,
        object_id: u64,
        definitions: &OverworldObjectDefinitions,
        spell_definitions: &SpellDefinitions,
    ) -> Option<String> {
        let type_id = self.type_id(object_id)?;
        let definition = definitions.get(type_id)?;
        Some(self.render_template(object_id, &definition.description, spell_definitions))
    }

    pub fn resolved_spell_id(
        &self,
        object_id: u64,
        definitions: &OverworldObjectDefinitions,
        spell_definitions: &SpellDefinitions,
    ) -> Option<String> {
        let type_id = self.type_id(object_id)?;
        let definition = definitions.get(type_id)?;
        definition
            .spell_id
            .as_ref()
            .map(|template| self.render_template(object_id, template, spell_definitions))
    }

    fn render_template(
        &self,
        object_id: u64,
        template: &str,
        spell_definitions: &SpellDefinitions,
    ) -> String {
        let Some(properties) = self.properties(object_id) else {
            return template.to_owned();
        };

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
            rendered.push_str(
                &resolve_template_expression(expression, properties, spell_definitions)
                    .unwrap_or_else(|| format!("{{{expression}}}")),
            );
            rest = &after_start[end + 1..];
        }

        rendered.push_str(rest);
        rendered
    }
}

fn resolve_template_expression(
    expression: &str,
    properties: &ObjectProperties,
    spell_definitions: &SpellDefinitions,
) -> Option<String> {
    let property = expression.strip_prefix("properties.")?;
    if let Some(property_name) = property.strip_suffix(".name") {
        let property_value = properties.get(property_name)?;
        let spell = spell_definitions.get(property_value)?;
        return Some(spell.name.clone());
    }

    properties.get(property).cloned()
}
