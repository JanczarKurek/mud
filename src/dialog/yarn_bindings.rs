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

use crate::dialog::resources::PlayerInventorySnapshots;

/// Installs runner-local functions. Called once per `DialogueRunner` at
/// session creation; `player_id` is captured in the closures so every Yarn
/// query reads the *acting* player's state.
pub fn install(
    runner: &mut DialogueRunner,
    _commands: &mut Commands,
    snapshots: &PlayerInventorySnapshots,
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
}
