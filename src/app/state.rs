use bevy::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, States)]
pub enum ClientAppState {
    #[default]
    TitleScreen,
    /// TCP-only: credentials have been entered; we're waiting for the server
    /// to accept the login/register. Transitions to `AssetSync` on success or
    /// back to `TitleScreen` on failure.
    Authenticating,
    AssetSync,
    InGame,
    MapEditor,
}

/// Runtime switch toggled by the diagnostics overlay (F8) so we can compare
/// frame-time spikes with simulation systems disabled vs. enabled. Lives in
/// `app::state` rather than `diagnostics` so server-side plugins can read it
/// without a circular module dep — the resource is only ever inserted by
/// `DiagnosticsPlugin` (client modes), so the headless server sees `None`.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct DiagnosticPause {
    pub simulation: bool,
}

/// System condition: true when the world simulation should tick.
///
/// In `EmbeddedClient` mode the state machine is present; simulation runs only
/// in `InGame`. In `HeadlessServer` mode there is no `ClientAppState` resource,
/// so the condition defaults to `true` (always simulate).
pub fn simulation_active(
    state: Option<Res<State<ClientAppState>>>,
    pause: Option<Res<DiagnosticPause>>,
) -> bool {
    if pause.is_some_and(|p| p.simulation) {
        return false;
    }
    state.map_or(true, |s| *s.get() == ClientAppState::InGame)
}
