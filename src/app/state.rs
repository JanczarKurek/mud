use bevy::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, States)]
pub enum ClientAppState {
    #[default]
    TitleScreen,
    InGame,
    MapEditor,
}

/// System condition: true when the world simulation should tick.
///
/// In `EmbeddedClient` mode the state machine is present; simulation runs only
/// in `InGame`. In `HeadlessServer` mode there is no `ClientAppState` resource,
/// so the condition defaults to `true` (always simulate).
pub fn simulation_active(state: Option<Res<State<ClientAppState>>>) -> bool {
    state.map_or(true, |s| *s.get() == ClientAppState::InGame)
}
