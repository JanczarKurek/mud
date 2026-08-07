use bevy::prelude::*;

/// Server-side component attached to an overworld object whose definition has
/// `dialog_node`. The inner string is the Yarn node the runner should start at
/// when a player talks to this object. Projection surfaces the presence of
/// this component to clients as `ClientWorldObjectState::has_dialog` so the
/// context menu can offer "Talk" without needing the runner itself.
#[derive(Clone, Component, Debug)]
pub struct DialogNode(pub String);

/// Server-side marker attached to a spawned `DialogueRunner` entity so systems
/// can locate it by session id without scanning every runner.
#[derive(Component, Debug)]
pub struct DialogSession {
    pub session_id: u64,
    /// The `PlayerId.0` of the character driving the dialog.
    pub player_id: u64,
    /// The object id of the NPC being talked to (used for range checks /
    /// cleanup if the NPC despawns).
    pub npc_object_id: u64,
}
