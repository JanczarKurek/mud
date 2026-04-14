use mud2::magic::resources::SpellDefinition;
use mud2::world::map_layout::SpaceDefinition;
use mud2::world::object_definitions::OverworldObjectDefinition;
use schemars::schema_for;

fn write_schema<T: schemars::JsonSchema>(name: &str) {
    let schema = schema_for!(T);
    let json = serde_json::to_string_pretty(&schema).expect("Failed to serialize schema");
    let path = format!("assets/schemas/{name}.schema.json");
    std::fs::write(&path, json).unwrap_or_else(|e| panic!("Failed to write {path}: {e}"));
    println!("Wrote {path}");
}

fn main() {
    std::fs::create_dir_all("assets/schemas").expect("Failed to create assets/schemas/");
    write_schema::<SpaceDefinition>("map_layout");
    write_schema::<OverworldObjectDefinition>("object_definition");
    write_schema::<SpellDefinition>("spell_definition");
}
