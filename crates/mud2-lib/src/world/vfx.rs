//! Data-driven visual effect (VFX) definitions.
//!
//! A `VfxDefinition` describes a transient overlay animation (blood splash,
//! cast flash, hit flash, …) or a sticky-buff overlay (shield bubble, sleep
//! Zs). Each lives in its own directory under `assets/vfx/<id>/metadata.yaml`
//! alongside its sprite-sheet PNG, mirroring the layout of
//! `assets/overworld_objects/`.

use std::collections::HashMap;
use std::fs;

use bevy::log::info;
use bevy::prelude::*;
use serde::Deserialize;

use crate::assets::AssetResolver;
use crate::world::object_definitions::AnimationSheetDef;

#[derive(Clone, Debug, Deserialize)]
pub struct VfxDefinition {
    /// Sprite-sheet animation. Must contain a clip named `play`. For one-shots
    /// that clip should set `looping: false` so the frame cycler holds on the
    /// last frame until `Ttl` despawns the entity. Sticky overlays set
    /// `looping: true` on `play`.
    pub animation: AnimationSheetDef,
    /// One-shot duration in seconds. When omitted the spawner falls back to
    /// `frame_count / fps` of the `play` clip. Ignored for sticky overlays
    /// (which have no `Ttl`).
    #[serde(default)]
    pub duration_seconds: Option<f32>,
    /// Multiplier on the rendered sprite size relative to its native frame
    /// dimensions. `None` ⇒ 1.0.
    #[serde(default)]
    pub scale: Option<f32>,
    /// Pixel offset above the anchor (positive = up). Used to lift effects off
    /// the ground onto the target's center / head.
    #[serde(default)]
    pub z_offset_pixels: Option<f32>,
    /// When true this effect is a looping overlay (no `Ttl`). Sticky-buff
    /// definitions opt in.
    #[serde(default)]
    pub looping: bool,
}

#[derive(Resource, Default)]
pub struct VfxDefinitions {
    definitions: HashMap<String, VfxDefinition>,
}

impl VfxDefinitions {
    pub fn load_from_disk() -> Self {
        let resolver = AssetResolver::new();
        let mut definitions = HashMap::new();
        for scan_dir in resolver.scan_dirs("vfx") {
            info!("loading vfx definitions from {}", scan_dir.display());
            let Ok(entries) = fs::read_dir(&scan_dir) else {
                continue;
            };
            for entry in entries {
                let entry = entry.expect("Failed to read vfx directory entry");
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(id) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let metadata_path = path.join("metadata.yaml");
                if !metadata_path.is_file() {
                    continue;
                }
                let yaml = fs::read_to_string(&metadata_path).unwrap_or_else(|error| {
                    panic!(
                        "Failed to read vfx metadata {}: {error}",
                        metadata_path.display()
                    )
                });
                let def = serde_yaml::from_str::<VfxDefinition>(&yaml).unwrap_or_else(|error| {
                    panic!(
                        "Failed to parse vfx metadata {}: {error}",
                        metadata_path.display()
                    )
                });
                definitions.insert(id.to_owned(), def);
            }
        }
        Self { definitions }
    }

    pub fn get(&self, id: &str) -> Option<&VfxDefinition> {
        self.definitions.get(id)
    }
}

impl VfxDefinition {
    /// Resolves the duration of a one-shot effect: explicit `duration_seconds`
    /// when set, otherwise the natural length of the `play` clip
    /// (`frame_count / fps`). Falls back to 0.5s if neither is meaningful.
    pub fn resolved_duration_seconds(&self) -> f32 {
        if let Some(secs) = self.duration_seconds {
            return secs.max(0.05);
        }
        if let Some(play) = self.animation.clips.get("play") {
            if play.fps > 0.0 && play.frame_count > 0 {
                return play.frame_count as f32 / play.fps;
            }
        }
        0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_authored_vfx_parse_from_disk() {
        let defs = VfxDefinitions::load_from_disk();
        for expected in [
            "blood_splash",
            "cast_flash",
            "hit_flash",
            "heal_sparkle",
            "death_poof",
            "teleport_flash",
            "shield_bubble",
            "bless_aura",
            "sleep_zs",
            "slow_drag",
            "glimmer_aura",
            "haste_streaks",
        ] {
            let def = defs
                .get(expected)
                .unwrap_or_else(|| panic!("missing vfx definition: {expected}"));
            assert!(
                def.animation.clips.contains_key("play"),
                "vfx '{expected}' must declare a 'play' clip"
            );
        }
    }

    #[test]
    fn one_shots_are_not_looping_and_overlays_are() {
        let defs = VfxDefinitions::load_from_disk();
        for one_shot in ["blood_splash", "cast_flash", "hit_flash", "death_poof"] {
            let def = defs.get(one_shot).unwrap();
            assert!(!def.looping, "{one_shot} should not be a looping overlay");
            assert!(
                !def.animation.clips["play"].looping,
                "{one_shot} 'play' clip should be non-looping"
            );
        }
        for overlay in ["shield_bubble", "bless_aura", "sleep_zs"] {
            let def = defs.get(overlay).unwrap();
            assert!(def.looping, "{overlay} should be a looping overlay");
            assert!(
                def.animation.clips["play"].looping,
                "{overlay} 'play' clip should loop"
            );
        }
    }
}
