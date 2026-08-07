//! Rust-registered Yarn commands and functions.
//!
//! Flag semantics (`<<set $flag to true>>` / `<<if $flag>>`) are handled by
//! Yarn's built-in variable syntax backed by the per-character
//! `MemoryVariableStorage` installed at runner creation — no custom commands
//! are needed for them. `give_item` / `take_item` are intercepted via an
//! `On<ExecuteCommand>` observer (see `systems::handle_yarn_item_commands`)
//! rather than registered here, because system-backed commands receive `In<T>`
//! but lose the source runner entity, which we need to resolve the acting
//! player.

use bevy::prelude::*;
use bevy_yarnspinner::prelude::*;

use crate::dialog::resources::{
    PlayerInventorySnapshots, PlayerSkillSnapshots, PlayerStashSnapshots,
};

/// Installs runner-local functions. Called once per `DialogueRunner` at
/// session creation; `player_id` is captured in the closures so every Yarn
/// query reads the *acting* player's state.
pub fn install(
    runner: &mut DialogueRunner,
    _commands: &mut Commands,
    snapshots: &PlayerInventorySnapshots,
    stash_snapshots: &PlayerStashSnapshots,
    skill_snapshots: &PlayerSkillSnapshots,
    player_id: u64,
) {
    let snapshot = snapshots.by_player.clone();
    runner
        .library_mut()
        .add_function("has_item", move |type_id: String, count: f32| -> bool {
            let guard = snapshot.read().expect("inventory snapshot RwLock poisoned");
            guard
                .get(&player_id)
                .and_then(|per_player| per_player.get(&type_id))
                .is_some_and(|total| *total >= count.max(0.0) as u32)
        });

    // --- Stash readers --------------------------------------------------
    //
    // Yarn's static-typed library functions force us to expose one getter
    // per return type (`*_str`, `*_num`, `*_bool`). Each returns a sensible
    // default when the key is missing or the JSON value is the wrong shape.
    // Authors gate with `stash_has(key)` first when they need to distinguish
    // "absent" from "present but wrong type".

    let stash = stash_snapshots.by_player.clone();
    runner
        .library_mut()
        .add_function("stash_has", move |key: String| -> bool {
            let guard = stash.read().expect("stash snapshot RwLock poisoned");
            guard
                .get(&player_id)
                .map(|entries| entries.contains_key(&key))
                .unwrap_or(false)
        });

    let stash = stash_snapshots.by_player.clone();
    runner
        .library_mut()
        .add_function("stash_get_str", move |key: String| -> String {
            let guard = stash.read().expect("stash snapshot RwLock poisoned");
            guard
                .get(&player_id)
                .and_then(|entries| entries.get(&key))
                .and_then(|value| value.as_str().map(|s| s.to_owned()))
                .unwrap_or_default()
        });

    let stash = stash_snapshots.by_player.clone();
    runner
        .library_mut()
        .add_function("stash_get_num", move |key: String| -> f32 {
            let guard = stash.read().expect("stash snapshot RwLock poisoned");
            guard
                .get(&player_id)
                .and_then(|entries| entries.get(&key))
                .and_then(|value| value.as_f64())
                .map(|n| n as f32)
                .unwrap_or(0.0)
        });

    let stash = stash_snapshots.by_player.clone();
    runner
        .library_mut()
        .add_function("stash_get_bool", move |key: String| -> bool {
            let guard = stash.read().expect("stash snapshot RwLock poisoned");
            guard
                .get(&player_id)
                .and_then(|entries| entries.get(&key))
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        });

    // skill_rank("Persuasion") → returns the player's current rank in that
    // skill, or 0 if the name doesn't match a known skill. Yarn passes
    // numbers as f32; return f32 so callers can compare with `>` directly.
    let skills = skill_snapshots.by_player.clone();
    runner
        .library_mut()
        .add_function("skill_rank", move |name: String| -> f32 {
            let Some(skill) = crate::player::skills::Skill::from_label(&name) else {
                return 0.0;
            };
            let guard = skills.read().expect("skill snapshot RwLock poisoned");
            guard
                .get(&player_id)
                .map(|ranks| ranks[skill.index()] as f32)
                .unwrap_or(0.0)
        });
}
