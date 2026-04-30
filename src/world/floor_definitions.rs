use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::assets::AssetResolver;

pub type FloorTypeId = String;

/// Canonical key for a transition tileset: `(low, high)` where `low` is the
/// lower-priority floor type (alphabetical id tiebreak on equal priority).
pub type TransitionPairKey = (FloorTypeId, FloorTypeId);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct FloorTilesetDefinition {
    pub id: FloorTypeId,
    pub name: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_tile_size_px")]
    pub tile_size_px: u32,
    #[serde(default)]
    pub atlas_path: Option<String>,
    pub debug_color: [u8; 3],
    /// Reserved for upper-storey floors; unused at z=0 in this slice.
    #[serde(default)]
    pub occludes_floor_above: bool,
    /// Reserved; ground floor is always walkable.
    #[serde(default = "default_true")]
    pub walkable_surface: bool,
    /// Per-bitmask variant weights. Key = corner-bitmask in `1..=15`
    /// (NW=1, NE=2, SW=4, SE=8). Value = list of positive weights, one per
    /// available variant of that bitmask in the atlas. Variant 0 occupies
    /// rows 0..=3 of the atlas (the base block); variant `i` occupies rows
    /// `4*i..=4*i+3`. Bitmasks omitted from the map have a single variant.
    #[serde(default)]
    pub variants: HashMap<u8, Vec<u32>>,
}

const fn default_tile_size_px() -> u32 {
    16
}

const fn default_true() -> bool {
    true
}

impl FloorTilesetDefinition {
    pub fn debug_color(&self) -> Color {
        Color::srgb_u8(
            self.debug_color[0],
            self.debug_color[1],
            self.debug_color[2],
        )
    }

    pub fn variant_weights(&self, mask: u8) -> &[u32] {
        const SINGLE: &[u32] = &[1];
        self.variants
            .get(&mask)
            .map(|v| v.as_slice())
            .unwrap_or(SINGLE)
    }

    pub fn max_variants(&self) -> usize {
        self.variants
            .values()
            .map(|v| v.len())
            .max()
            .unwrap_or(1)
            .max(1)
    }
}

/// A blended atlas drawn between two floor types where they meet at a corner.
/// Keyed by canonical `(low, high)` floor ids — `low` is the lower-priority
/// floor (alphabetical id tiebreak on equal priority). The atlas paints the
/// high-side pixels with a feathered border onto a solid low base; see
/// `floor_render::spawn_render_cells_at_corner` for how it's composited.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct FloorTransitionDefinition {
    pub low: FloorTypeId,
    pub high: FloorTypeId,
    #[serde(default = "default_tile_size_px")]
    pub tile_size_px: u32,
    pub atlas_path: String,
    /// Per-bitmask variant weights, indexed by the **high-side** corner
    /// bitmask (bits set where the high floor type sits).
    #[serde(default)]
    pub variants: HashMap<u8, Vec<u32>>,
}

impl FloorTransitionDefinition {
    pub fn variant_weights(&self, mask: u8) -> &[u32] {
        const SINGLE: &[u32] = &[1];
        self.variants
            .get(&mask)
            .map(|v| v.as_slice())
            .unwrap_or(SINGLE)
    }

    pub fn max_variants(&self) -> usize {
        self.variants
            .values()
            .map(|v| v.len())
            .max()
            .unwrap_or(1)
            .max(1)
    }
}

#[derive(Resource, Default, Clone, Debug)]
pub struct FloorTilesetDefinitions {
    by_id: HashMap<FloorTypeId, FloorTilesetDefinition>,
    transitions: HashMap<TransitionPairKey, FloorTransitionDefinition>,
}

impl FloorTilesetDefinitions {
    pub fn load_from_disk() -> Self {
        let resolver = AssetResolver::new();
        let scan_dirs = resolver.scan_dirs("floors");
        let mut by_id = HashMap::new();

        for scan_dir in &scan_dirs {
            info!(
                "loading floor tileset definitions from {}",
                scan_dir.display()
            );
            let Ok(entries) = fs::read_dir(scan_dir) else {
                continue;
            };
            for entry in entries {
                let entry = entry.expect("Failed to read floor tileset directory entry");
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(directory_name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                // Transitions live under floors/transitions/ and are loaded in pass 2.
                if directory_name == "transitions" {
                    continue;
                }
                let metadata_path = path.join("metadata.yaml");
                if !metadata_path.is_file() {
                    continue;
                }
                let yaml = fs::read_to_string(&metadata_path).unwrap_or_else(|error| {
                    panic!(
                        "Failed to read floor tileset metadata {}: {error}",
                        metadata_path.display()
                    )
                });
                let mut def: FloorTilesetDefinition =
                    serde_yaml::from_str(&yaml).unwrap_or_else(|error| {
                        panic!(
                            "Failed to parse floor tileset metadata {}: {error}",
                            metadata_path.display()
                        )
                    });
                if def.id.trim().is_empty() {
                    def.id = directory_name.to_owned();
                }
                assert!(
                    def.id == directory_name,
                    "Floor tileset id '{}' does not match directory name '{}'",
                    def.id,
                    directory_name
                );
                assert!(
                    def.tile_size_px > 0,
                    "Floor tileset '{}' has tile_size_px = 0",
                    def.id
                );
                for (mask, weights) in &def.variants {
                    assert!(
                        (1..=15).contains(mask),
                        "Floor tileset '{}': variant key {} out of range 1..=15",
                        def.id,
                        mask
                    );
                    assert!(
                        !weights.is_empty(),
                        "Floor tileset '{}': variant {} has empty weights list",
                        def.id,
                        mask
                    );
                    assert!(
                        weights.iter().all(|w| *w > 0),
                        "Floor tileset '{}': variant {} has a zero weight",
                        def.id,
                        mask
                    );
                }
                info!(
                    "floor tileset '{}': priority={}, atlas={:?}, tile_size_px={}, max_variants={}",
                    def.id,
                    def.priority,
                    def.atlas_path,
                    def.tile_size_px,
                    def.max_variants(),
                );
                by_id.insert(def.id.clone(), def);
            }
        }

        let transitions = load_transitions(&scan_dirs, &by_id);

        Self { by_id, transitions }
    }

    pub fn get(&self, id: &str) -> Option<&FloorTilesetDefinition> {
        self.by_id.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.by_id.keys().map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = &FloorTilesetDefinition> {
        self.by_id.values()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }

    pub fn transitions(&self) -> impl Iterator<Item = (&TransitionPairKey, &FloorTransitionDefinition)> {
        self.transitions.iter()
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        by_id: HashMap<FloorTypeId, FloorTilesetDefinition>,
        transitions: HashMap<TransitionPairKey, FloorTransitionDefinition>,
    ) -> Self {
        Self { by_id, transitions }
    }

    /// Canonicalises a pair of floor ids into `(low, high)` order: lower
    /// priority first, with alphabetical id tiebreak on equal priority.
    /// Returns `None` if either id is unknown to the loader.
    pub fn canonicalise_pair<'a>(
        &self,
        a: &'a FloorTypeId,
        b: &'a FloorTypeId,
    ) -> Option<(&'a FloorTypeId, &'a FloorTypeId)> {
        let pa = self.by_id.get(a)?.priority;
        let pb = self.by_id.get(b)?.priority;
        Some(match (pa, a.as_str()).cmp(&(pb, b.as_str())) {
            Ordering::Greater => (b, a),
            _ => (a, b),
        })
    }

    /// Looks up a transition tileset for the unordered pair `(a, b)`. Returns
    /// the canonical `(low, high)` floor ids alongside the transition
    /// definition, or `None` if no transition is authored for the pair.
    pub fn transition_for<'a>(
        &'a self,
        a: &'a FloorTypeId,
        b: &'a FloorTypeId,
    ) -> Option<(
        &'a FloorTypeId,
        &'a FloorTypeId,
        &'a FloorTransitionDefinition,
    )> {
        let (low, high) = self.canonicalise_pair(a, b)?;
        let key = (low.clone(), high.clone());
        self.transitions.get(&key).map(|def| (low, high, def))
    }
}

fn load_transitions(
    scan_dirs: &[std::path::PathBuf],
    by_id: &HashMap<FloorTypeId, FloorTilesetDefinition>,
) -> HashMap<TransitionPairKey, FloorTransitionDefinition> {
    let mut out: HashMap<TransitionPairKey, FloorTransitionDefinition> = HashMap::new();

    for scan_dir in scan_dirs {
        let transitions_dir = scan_dir.join("transitions");
        let Ok(entries) = fs::read_dir(&transitions_dir) else {
            continue;
        };
        info!(
            "loading floor transition definitions from {}",
            transitions_dir.display()
        );
        for entry in entries {
            let entry = entry.expect("Failed to read floor transitions directory entry");
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(directory_name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let metadata_path = path.join("metadata.yaml");
            if !metadata_path.is_file() {
                continue;
            }
            let yaml = fs::read_to_string(&metadata_path).unwrap_or_else(|error| {
                panic!(
                    "Failed to read floor transition metadata {}: {error}",
                    metadata_path.display()
                )
            });
            let def: FloorTransitionDefinition =
                serde_yaml::from_str(&yaml).unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse floor transition metadata {}: {error}",
                        metadata_path.display()
                    )
                });

            assert!(
                def.low != def.high,
                "Floor transition '{}': low and high must differ",
                directory_name
            );
            let low_def = by_id.get(&def.low).unwrap_or_else(|| {
                panic!(
                    "Floor transition '{}': unknown low floor type '{}'",
                    directory_name, def.low
                )
            });
            let high_def = by_id.get(&def.high).unwrap_or_else(|| {
                panic!(
                    "Floor transition '{}': unknown high floor type '{}'",
                    directory_name, def.high
                )
            });

            // Canonical order: low's (priority, id) <= high's. Catches authoring
            // mistakes where the YAML swaps low and high.
            let low_key = (low_def.priority, def.low.as_str());
            let high_key = (high_def.priority, def.high.as_str());
            assert!(
                low_key <= high_key,
                "Floor transition '{}': low '{}' (priority {}) must precede high '{}' (priority {}) in canonical order (priority asc, id alphabetical tiebreak)",
                directory_name, def.low, low_def.priority, def.high, high_def.priority
            );

            let expected_dir = format!("{}__{}", def.low, def.high);
            assert!(
                directory_name == expected_dir,
                "Floor transition directory '{}' does not match metadata pair '{}'",
                directory_name,
                expected_dir
            );

            assert!(
                def.tile_size_px == low_def.tile_size_px
                    && def.tile_size_px == high_def.tile_size_px,
                "Floor transition '{}': tile_size_px {} must match both endpoints (low={}, high={})",
                directory_name,
                def.tile_size_px,
                low_def.tile_size_px,
                high_def.tile_size_px
            );

            for (mask, weights) in &def.variants {
                assert!(
                    (1..=15).contains(mask),
                    "Floor transition '{}': variant key {} out of range 1..=15",
                    directory_name,
                    mask
                );
                assert!(
                    !weights.is_empty(),
                    "Floor transition '{}': variant {} has empty weights list",
                    directory_name,
                    mask
                );
                assert!(
                    weights.iter().all(|w| *w > 0),
                    "Floor transition '{}': variant {} has a zero weight",
                    directory_name,
                    mask
                );
            }

            info!(
                "floor transition '{}__{}': atlas={}, tile_size_px={}, max_variants={}",
                def.low,
                def.high,
                def.atlas_path,
                def.tile_size_px,
                def.max_variants(),
            );
            out.insert((def.low.clone(), def.high.clone()), def);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(id: &str, priority: i32) -> FloorTilesetDefinition {
        FloorTilesetDefinition {
            id: id.to_owned(),
            name: id.to_owned(),
            priority,
            tile_size_px: 16,
            atlas_path: None,
            debug_color: [0, 0, 0],
            occludes_floor_above: false,
            walkable_surface: true,
            variants: HashMap::new(),
        }
    }

    fn tr(low: &str, high: &str) -> FloorTransitionDefinition {
        FloorTransitionDefinition {
            low: low.to_owned(),
            high: high.to_owned(),
            tile_size_px: 16,
            atlas_path: format!("floors/transitions/{low}__{high}/tileset.png"),
            variants: HashMap::new(),
        }
    }

    fn defs_with(
        floors: &[(&str, i32)],
        transitions: &[(&str, &str)],
    ) -> FloorTilesetDefinitions {
        let mut by_id = HashMap::new();
        for (id, priority) in floors {
            by_id.insert((*id).to_owned(), ts(id, *priority));
        }
        let mut tmap = HashMap::new();
        for (low, high) in transitions {
            tmap.insert(((*low).to_owned(), (*high).to_owned()), tr(low, high));
        }
        FloorTilesetDefinitions {
            by_id,
            transitions: tmap,
        }
    }

    #[test]
    fn canonicalise_pair_orders_by_priority() {
        let defs = defs_with(&[("grass", 0), ("brick", 1)], &[]);
        let g = "grass".to_owned();
        let b = "brick".to_owned();
        let (low, high) = defs.canonicalise_pair(&g, &b).unwrap();
        assert_eq!(low, "grass");
        assert_eq!(high, "brick");
        let (low, high) = defs.canonicalise_pair(&b, &g).unwrap();
        assert_eq!(low, "grass");
        assert_eq!(high, "brick");
    }

    #[test]
    fn canonicalise_pair_alphabetical_tiebreak() {
        let defs = defs_with(&[("xeno", 5), ("alpha", 5)], &[]);
        let x = "xeno".to_owned();
        let a = "alpha".to_owned();
        let (low, high) = defs.canonicalise_pair(&x, &a).unwrap();
        assert_eq!(low, "alpha");
        assert_eq!(high, "xeno");
    }

    #[test]
    fn transition_lookup_is_order_insensitive() {
        let defs = defs_with(&[("grass", 0), ("brick", 1)], &[("grass", "brick")]);
        let g = "grass".to_owned();
        let b = "brick".to_owned();
        let (low_a, high_a, _) = defs.transition_for(&g, &b).unwrap();
        let (low_b, high_b, _) = defs.transition_for(&b, &g).unwrap();
        assert_eq!(low_a, "grass");
        assert_eq!(high_a, "brick");
        assert_eq!(low_b, "grass");
        assert_eq!(high_b, "brick");
    }

    #[test]
    fn transition_lookup_returns_none_when_unauthored() {
        let defs = defs_with(&[("grass", 0), ("brick", 1)], &[]);
        let g = "grass".to_owned();
        let b = "brick".to_owned();
        assert!(defs.transition_for(&g, &b).is_none());
    }

    #[test]
    fn canonicalise_pair_unknown_id_returns_none() {
        let defs = defs_with(&[("grass", 0)], &[]);
        let g = "grass".to_owned();
        let unknown = "unknown".to_owned();
        assert!(defs.canonicalise_pair(&g, &unknown).is_none());
    }

    /// Exercises the real on-disk loader. Catches YAML/path/canonicalisation
    /// regressions in the bundled `assets/floors/transitions/` folder.
    #[test]
    fn loads_smoke_test_transition_from_disk() {
        let defs = FloorTilesetDefinitions::load_from_disk();
        let g = "grass".to_owned();
        let c = "cobblestone".to_owned();
        let (low, high, def) = defs
            .transition_for(&g, &c)
            .expect("grass__cobblestone smoke-test transition should be loaded");
        assert_eq!(low, "grass");
        assert_eq!(high, "cobblestone");
        assert_eq!(def.tile_size_px, 16);
    }
}
