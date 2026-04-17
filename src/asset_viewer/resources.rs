use bevy::prelude::*;

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetKind {
    #[default]
    Object,
    Spell,
}

#[derive(Resource, Default)]
pub struct ViewerState {
    pub filter: String,
    pub filter_focused: bool,
    pub selected_id: Option<String>,
    pub selected_kind: AssetKind,
}

#[derive(Resource, Default)]
pub struct PreviewState {
    pub current_clip: Option<String>,
    pub preview_entity: Option<Entity>,
}

/// Editable view of the selected asset's raw YAML (pre-inheritance).
#[derive(Resource, Default)]
pub struct InspectorBuffer {
    pub asset_id: Option<String>,
    pub kind: AssetKind,
    pub raw_value: Option<serde_yaml::Value>,
    pub fields: Vec<InspectorField>,
    pub editing_index: Option<usize>,
    pub edit_text: String,
    pub dirty: bool,
}

#[derive(Clone, Debug)]
pub struct InspectorField {
    /// Dot-separated display path, e.g. "render.z_index"
    pub display_path: String,
    pub display_value: String,
    /// Path components for navigation into raw_value
    pub yaml_path: Vec<String>,
}

impl InspectorBuffer {
    pub fn load_object(&mut self, id: &str) {
        let path = format!("assets/overworld_objects/{}/metadata.yaml", id);
        self.load_from_path(id, AssetKind::Object, &path);
    }

    pub fn load_spell(&mut self, id: &str) {
        let path = format!("assets/spells/{}.yaml", id);
        self.load_from_path(id, AssetKind::Spell, &path);
    }

    fn load_from_path(&mut self, id: &str, kind: AssetKind, path: &str) {
        self.asset_id = Some(id.to_owned());
        self.kind = kind;
        self.editing_index = None;
        self.edit_text.clear();
        self.dirty = false;

        match std::fs::read_to_string(path) {
            Ok(yaml_str) => match serde_yaml::from_str::<serde_yaml::Value>(&yaml_str) {
                Ok(value) => {
                    self.fields = flatten_yaml(&value, "");
                    self.raw_value = Some(value);
                }
                Err(e) => {
                    bevy::log::error!("Failed to parse YAML for {}: {}", id, e);
                    self.raw_value = None;
                    self.fields.clear();
                }
            },
            Err(e) => {
                bevy::log::error!("Failed to read YAML for {}: {}", id, e);
                self.raw_value = None;
                self.fields.clear();
            }
        }
    }

    /// Commit the currently-edited field into raw_value and regenerate fields.
    pub fn commit_edit(&mut self) {
        let Some(idx) = self.editing_index else {
            return;
        };
        let text = std::mem::take(&mut self.edit_text);
        self.editing_index = None;

        if let Some(field) = self.fields.get(idx).cloned() {
            if let Some(value) = &mut self.raw_value {
                apply_edit(value, &field.yaml_path, &text);
            }
        }

        if let Some(value) = &self.raw_value {
            self.fields = flatten_yaml(value, "");
        }
        self.dirty = true;
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.commit_edit();

        let id = self.asset_id.clone().ok_or("No asset selected")?;
        let value = self.raw_value.as_ref().ok_or("No value to save")?;

        let yaml_str =
            serde_yaml::to_string(value).map_err(|e| format!("Serialize error: {}", e))?;

        let path = match self.kind {
            AssetKind::Object => format!("assets/overworld_objects/{}/metadata.yaml", id),
            AssetKind::Spell => format!("assets/spells/{}.yaml", id),
        };

        std::fs::write(&path, yaml_str).map_err(|e| format!("Write error for {}: {}", path, e))?;

        self.dirty = false;
        Ok(())
    }
}

fn flatten_yaml(value: &serde_yaml::Value, prefix: &str) -> Vec<InspectorField> {
    let mut out = Vec::new();
    flatten_yaml_into(value, prefix, &mut out);
    out
}

fn flatten_yaml_into(value: &serde_yaml::Value, prefix: &str, out: &mut Vec<InspectorField>) {
    let serde_yaml::Value::Mapping(map) = value else {
        return;
    };
    for (k, v) in map {
        let key_str = k.as_str().unwrap_or("?");
        let path = if prefix.is_empty() {
            key_str.to_string()
        } else {
            format!("{}.{}", prefix, key_str)
        };
        let yaml_path: Vec<String> = path.split('.').map(str::to_owned).collect();

        match v {
            serde_yaml::Value::Mapping(_) => {
                flatten_yaml_into(v, &path, out);
            }
            serde_yaml::Value::Sequence(seq) => {
                let display = seq
                    .iter()
                    .map(|item| match item {
                        serde_yaml::Value::Number(n) => n.to_string(),
                        serde_yaml::Value::String(s) => s.clone(),
                        serde_yaml::Value::Bool(b) => b.to_string(),
                        _ => "?".to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push(InspectorField {
                    display_path: path,
                    display_value: display,
                    yaml_path,
                });
            }
            _ => {
                let display = match v {
                    serde_yaml::Value::Null => "~".to_string(),
                    serde_yaml::Value::Bool(b) => b.to_string(),
                    serde_yaml::Value::Number(n) => n.to_string(),
                    serde_yaml::Value::String(s) => s.clone(),
                    _ => "?".to_string(),
                };
                out.push(InspectorField {
                    display_path: path,
                    display_value: display,
                    yaml_path,
                });
            }
        }
    }
}

fn apply_edit(root: &mut serde_yaml::Value, path: &[String], new_str: &str) {
    if path.is_empty() {
        return;
    }
    let serde_yaml::Value::Mapping(map) = root else {
        return;
    };
    let key = serde_yaml::Value::String(path[0].clone());

    if path.len() == 1 {
        if let Some(old) = map.get(&key).cloned() {
            let new_val = coerce_value(&old, new_str);
            map.insert(key, new_val);
        }
    } else if let Some(child) = map.get_mut(&key) {
        apply_edit(child, &path[1..], new_str);
    }
}

fn coerce_value(old: &serde_yaml::Value, new_str: &str) -> serde_yaml::Value {
    match old {
        serde_yaml::Value::Bool(_) => {
            serde_yaml::Value::Bool(new_str == "true" || new_str == "yes")
        }
        serde_yaml::Value::Number(n) => {
            if n.as_f64().is_some_and(|f| f.fract() != 0.0) {
                new_str
                    .parse::<f64>()
                    .map(serde_yaml::Value::from)
                    .unwrap_or_else(|_| serde_yaml::Value::String(new_str.to_owned()))
            } else {
                new_str
                    .parse::<i64>()
                    .map(serde_yaml::Value::from)
                    .unwrap_or_else(|_| {
                        new_str
                            .parse::<f64>()
                            .map(serde_yaml::Value::from)
                            .unwrap_or_else(|_| serde_yaml::Value::String(new_str.to_owned()))
                    })
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            let parts: Vec<&str> = new_str.split(',').map(str::trim).collect();
            let new_seq = parts
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    seq.get(i)
                        .map(|old_elem| coerce_value(old_elem, s))
                        .unwrap_or_else(|| serde_yaml::Value::String(s.to_string()))
                })
                .collect();
            serde_yaml::Value::Sequence(new_seq)
        }
        serde_yaml::Value::Null => {
            if new_str.is_empty() || new_str == "~" || new_str == "null" {
                serde_yaml::Value::Null
            } else {
                serde_yaml::Value::String(new_str.to_owned())
            }
        }
        _ => serde_yaml::Value::String(new_str.to_owned()),
    }
}
