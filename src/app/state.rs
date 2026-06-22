use bevy::prelude::*;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, States)]
pub enum ClientAppState {
    #[default]
    TitleScreen,
    /// Static credits + greetings screen, reachable from the title screen.
    /// Returns to `TitleScreen` via Back / Escape.
    About,
    /// TCP-only: credentials have been entered; we're waiting for the server
    /// to accept the login/register. Transitions to `CharacterSelect` on
    /// success or back to `TitleScreen` on failure.
    Authenticating,
    /// Account is authenticated; show the character roster + Create button.
    /// Transitions to `CharacterCreate` (user clicks Create new) or to
    /// `AssetSync` (user picks a character to play).
    CharacterSelect,
    /// Form for creating a new character (name + class + attributes).
    /// Transitions back to `CharacterSelect` on success or cancel.
    CharacterCreate,
    AssetSync,
    InGame,
    MapEditor,
}

/// EmbeddedClient-only: the `character_id` the user picked on the Character
/// Select screen. Read by `spawn_embedded_player_authoritative` on transition
/// to `InGame`. `None` means "pick the most recently played" (e.g. for the
/// very first frame before the user has interacted with the select screen).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct LocalSelectedCharacter {
    pub character_id: Option<i64>,
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

/// Runtime debug/dev-tooling switch, set once at startup from the `--debug`
/// CLI flag (or `MUD2_DEBUG` env) and never mutated afterwards. Gates the
/// main-menu "Clean game state" button, auto-creation of a default character,
/// and the in-game GM tools panel. Lives in `app::state` (like
/// [`DiagnosticPause`]) so any plugin can read it without a circular dep. When
/// the flag is absent the resource holds `false`, so all debug features are
/// inert. Callers read it directly via `Option<Res<DebugMode>>` and
/// `is_some_and(|m| m.0)`.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct DebugMode(pub bool);

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
    state.is_none_or(|s| *s.get() == ClientAppState::InGame)
}
