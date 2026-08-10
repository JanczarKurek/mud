//! Tag-based hostility: the data model behind `tags:` / `hostile_towards:` /
//! `flees_from:` in object YAML.
//!
//! Tags are free-form strings in YAML, interned once at startup into bits of
//! a `u64` so the per-tick detection filters stay a couple of integer ops.
//! The unified predicate is [`is_hostile_toward`]: faction enmity (the
//! pre-existing PlayerSide↔MonsterSide axis) OR tag overlap between the
//! aggressor's `hostile_towards` and the target's identity. Tag hostility is
//! asymmetric by design — a wolf is hostile toward `livestock`, the sheep
//! bears the tag without reciprocating.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::combat::components::CombatLeash;
use crate::npc::components::{Companion, Faction, HostileBehavior, Npc, PreyBehavior};
use crate::world::components::OverworldObject;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// A set of interned tags as a bitmask. Cheap to copy/compare; capacity 64
/// distinct tag strings game-wide (bit 0 reserved for the implicit `player`
/// identity).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TagMask(pub u64);

impl TagMask {
    pub const EMPTY: TagMask = TagMask(0);
    /// The implicit identity every player carries. Reserved at interner
    /// build time so `hostile_towards: [player]` works in YAML.
    pub const PLAYER: TagMask = TagMask(1);

    pub fn intersects(self, other: TagMask) -> bool {
        self.0 & other.0 != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// The reserved spelling of the implicit player tag.
pub const PLAYER_TAG: &str = "player";

/// String → bit interner, built once from every loaded object definition's
/// `tags` / `hostile_towards` / `flees_from` lists. A resource so spawn-time
/// resolution and (future) script spawns share one mapping.
#[derive(Resource, Default)]
pub struct TagInterner {
    bits: HashMap<String, u8>,
    /// Tags beyond the 64-bit capacity, warned once at build time and
    /// resolved to the empty mask thereafter.
    overflowed: Vec<String>,
}

impl TagInterner {
    /// Build from an iterator over every tag string mentioned anywhere.
    pub fn build<'a>(all_tags: impl Iterator<Item = &'a str>) -> Self {
        let mut interner = TagInterner::default();
        interner.bits.insert(PLAYER_TAG.to_owned(), 0);
        for tag in all_tags {
            if interner.bits.contains_key(tag) {
                continue;
            }
            let next = interner.bits.len() as u8;
            if next >= 64 {
                warn!(
                    "tag interner is full (64 tags); ignoring tag '{tag}' — \
                     creatures using it will not match on it"
                );
                interner.overflowed.push(tag.to_owned());
                continue;
            }
            interner.bits.insert(tag.to_owned(), next);
        }
        interner
    }

    /// Resolve a YAML tag list to a mask. Unknown tags resolve to nothing
    /// (they were either overflowed at build time or never declared).
    pub fn resolve(&self, tags: &[String]) -> TagMask {
        let mut mask = 0u64;
        for tag in tags {
            if let Some(bit) = self.bits.get(tag.as_str()) {
                mask |= 1 << bit;
            }
        }
        TagMask(mask)
    }
}

/// Per-NPC resolved tag data. Re-derived from the object definition at spawn
/// and on snapshot restore (like `Barks`) — never persisted.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct TagProfile {
    /// What this creature *is* (`tags:`).
    pub identity: TagMask,
    /// What it attacks on sight (`hostile_towards:`).
    pub hostile_towards: TagMask,
    /// What it runs from (`flees_from:`).
    pub flees_from: TagMask,
}

/// The one hostility predicate: is `(a_faction, a_hostile_towards)` hostile
/// toward `(b_faction, b_identity)`? Faction enmity is symmetric; the tag
/// gate is not. Used by NPC target acquisition and the per-viewer
/// `is_hostile` projection flag.
pub fn is_hostile_toward(
    a_faction: Faction,
    a_hostile_towards: TagMask,
    b_faction: Faction,
    b_identity: TagMask,
) -> bool {
    a_faction.is_enemy_of(b_faction) || a_hostile_towards.intersects(b_identity)
}

/// Detection numbers for a prey NPC whose definition has no `npc_behavior:`
/// block to borrow them from.
const DEFAULT_PREY_DETECT_TILES: i32 = 6;

/// Resolves the tag-model components for every freshly-added NPC, whatever
/// path spawned it (map placement, spawn group, summon, admin spawn, snapshot
/// restore — they all insert `Npc`, so `Added<Npc>` catches them all without
/// threading the interner through every spawn signature):
///
/// - `TagProfile` from the definition's `tags` / `hostile_towards` /
///   `flees_from` (only when any is non-empty),
/// - `PreyBehavior` when `flees_from` is non-empty,
/// - `HostileBehavior` + `CombatLeash` when `hostile_towards` is non-empty and
///   the spawn behavior didn't already attach them (a guard on a plain `roam`
///   still needs combat AI to fight monsters),
/// - `Faction`: an explicit definition `faction:` wins; otherwise tag-bearing
///   NPCs without a faction land on `Neutral` (targetable, nobody's faction
///   enemy). Companions are exempt from faction rewriting — their side is the
///   summoner's, set at spawn.
///
/// Runs one command-application after the spawn itself, which is well inside
/// the NPC's first AI step delay.
pub fn resolve_npc_tag_components(
    interner: Res<TagInterner>,
    definitions: Res<OverworldObjectDefinitions>,
    fresh: Query<
        (
            Entity,
            &OverworldObject,
            Option<&Faction>,
            Has<HostileBehavior>,
            Has<Companion>,
        ),
        Added<Npc>,
    >,
    mut commands: Commands,
) {
    for (entity, object, faction, has_hostile, is_companion) in &fresh {
        let Some(def) = definitions.get(&object.definition_id) else {
            continue;
        };
        let profile = TagProfile {
            identity: interner.resolve(&def.tags),
            hostile_towards: interner.resolve(&def.hostile_towards),
            flees_from: interner.resolve(&def.flees_from),
        };
        let has_tag_data = !profile.identity.is_empty()
            || !profile.hostile_towards.is_empty()
            || !profile.flees_from.is_empty();
        if has_tag_data {
            commands.entity(entity).insert(profile);
        }

        if !profile.flees_from.is_empty() {
            let (detect, los) = def
                .npc_behavior
                .as_ref()
                .map(|b| (b.detect_distance_tiles.max(1), b.requires_line_of_sight))
                .unwrap_or((DEFAULT_PREY_DETECT_TILES, true));
            commands.entity(entity).insert(PreyBehavior {
                detect_distance_tiles: detect,
                requires_line_of_sight: los,
            });
        }

        if !profile.hostile_towards.is_empty() && !has_hostile {
            if let Some(b) = def.npc_behavior.as_ref() {
                let detect = b.detect_distance_tiles.max(1);
                let disengage = b.disengage_distance_tiles.max(detect);
                commands.entity(entity).insert((
                    HostileBehavior {
                        detect_distance_tiles: detect,
                        disengage_distance_tiles: disengage,
                        alert_duration_seconds: b.alert_duration_seconds,
                        requires_line_of_sight: b.requires_line_of_sight,
                        perception: b.perception,
                    },
                    CombatLeash {
                        max_distance_tiles: disengage,
                    },
                ));
            }
        }

        if is_companion {
            continue;
        }
        if let Some(explicit) = def.faction {
            let explicit: Faction = explicit.into();
            if faction != Some(&explicit) {
                commands.entity(entity).insert(explicit);
            }
        } else if faction.is_none() && has_tag_data {
            commands.entity(entity).insert(Faction::Neutral);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interner_reserves_player_bit_and_interns_in_order() {
        let tags = ["beast", "predator", "beast", "livestock"];
        let interner = TagInterner::build(tags.into_iter());
        let player = interner.resolve(&[PLAYER_TAG.to_owned()]);
        assert_eq!(player, TagMask::PLAYER);
        let beast = interner.resolve(&["beast".to_owned()]);
        let predator = interner.resolve(&["predator".to_owned()]);
        assert_ne!(beast, TagMask::EMPTY);
        assert_ne!(predator, TagMask::EMPTY);
        assert!(!beast.intersects(predator));
        let both = interner.resolve(&["beast".to_owned(), "predator".to_owned()]);
        assert!(both.intersects(beast) && both.intersects(predator));
        // Unknown tag resolves to nothing rather than panicking.
        assert_eq!(interner.resolve(&["dragon".to_owned()]), TagMask::EMPTY);
    }

    #[test]
    fn interner_overflow_warns_and_ignores() {
        let many: Vec<String> = (0..80).map(|i| format!("tag{i}")).collect();
        let interner = TagInterner::build(many.iter().map(|s| s.as_str()));
        // First 63 custom tags fit (bit 0 is `player`); the rest resolve empty.
        assert_ne!(interner.resolve(&["tag0".to_owned()]), TagMask::EMPTY);
        assert_ne!(interner.resolve(&["tag62".to_owned()]), TagMask::EMPTY);
        assert_eq!(interner.resolve(&["tag63".to_owned()]), TagMask::EMPTY);
    }

    #[test]
    fn hostility_predicate_truth_table() {
        use Faction::*;
        let wolf_hostile = TagMask(0b10); // hostile_towards: livestock
        let sheep_identity = TagMask(0b10); // tags: [livestock]
        let none = TagMask::EMPTY;

        // Faction axis: only PlayerSide↔MonsterSide are enemies.
        assert!(is_hostile_toward(PlayerSide, none, MonsterSide, none));
        assert!(is_hostile_toward(MonsterSide, none, PlayerSide, none));
        assert!(!is_hostile_toward(PlayerSide, none, PlayerSide, none));
        assert!(!is_hostile_toward(Neutral, none, PlayerSide, none));
        assert!(!is_hostile_toward(Neutral, none, MonsterSide, none));
        assert!(!is_hostile_toward(MonsterSide, none, Neutral, none));

        // Tag axis is asymmetric: wolf → sheep, never sheep → wolf.
        assert!(is_hostile_toward(
            Neutral,
            wolf_hostile,
            Neutral,
            sheep_identity
        ));
        assert!(!is_hostile_toward(Neutral, none, Neutral, wolf_hostile));

        // Either gate suffices.
        assert!(is_hostile_toward(
            MonsterSide,
            none,
            PlayerSide,
            sheep_identity
        ));
        assert!(is_hostile_toward(
            PlayerSide,
            wolf_hostile,
            Neutral,
            sheep_identity
        ));
    }
}
