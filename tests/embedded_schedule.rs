//! Schedule-graph smoke test for the unified embedded plugin stack.
//!
//! `cargo check` cannot catch system-ordering contradictions — Bevy only
//! detects an unsolvable graph (cycles, "before a set it belongs to") when
//! the schedule is initialized on the first `App::update()`. The embedded
//! client additionally needs a window + GPU, so its graph never runs in CI
//! and a bad edge ships silently (this exact class crashed the GUI once:
//! a UI system ordered `.after(apply_game_ui_events)` — which now sits after
//! the whole loopback pipeline — while also `.before(CommandIntercept)`).
//!
//! This test rebuilds the EmbeddedClient plugin stack from `app/plugin.rs`
//! with the render/window layers replaced by headless stand-ins, then runs
//! one update: an unsolvable `Update` graph panics right here instead of on
//! the user's screen. Keep the plugin list in sync with the embedded arm of
//! `GameAppPlugin` when adding plugins there.

use bevy::prelude::*;
use mud2::app::plugin::AppRuntime;

mod common;
use common::unique_test_path;

#[test]
fn embedded_plugin_stack_schedule_solves() {
    let mut app = App::new();
    // Missing render-/window-only resources make systems fail parameter
    // validation in this headless build; downgrade those to warnings. An
    // unsolvable schedule graph still panics — it aborts schedule
    // initialization directly, bypassing this handler.
    app.set_error_handler(bevy::ecs::error::warn);
    app.insert_resource(AppRuntime::EmbeddedClient);
    app.insert_resource(mud2::app::state::DebugMode(true));

    // Headless stand-ins for the DefaultPlugins layers the embedded arm uses.
    app.add_plugins(MinimalPlugins);
    app.add_plugins(bevy::state::app::StatesPlugin);
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.add_plugins(bevy::input::InputPlugin);
    app.add_plugins(bevy::window::WindowPlugin {
        primary_window: None,
        exit_condition: bevy::window::ExitCondition::DontExit,
        ..Default::default()
    });
    app.add_plugins(bevy::a11y::AccessibilityPlugin);
    app.init_asset::<Image>();
    app.init_asset::<Font>();
    app.init_asset::<TextureAtlasLayout>();

    // The embedded arm of `GameAppPlugin` (crates/mud2-lib/src/app/plugin.rs),
    // minus DefaultPlugins (stubbed above) and the editor extension.
    app.add_plugins((
        mud2::game::GameServerPlugin,
        mud2::world::WorldServerPlugin,
        mud2::npc::NpcPlugin,
        mud2::player::PlayerServerPlugin,
        mud2::combat::CombatPlugin,
        mud2::magic::MagicServerPlugin,
        mud2::magic::MagicClientPlugin,
        mud2::crafting::CraftingServerPlugin,
        mud2::log::LogServerPlugin,
        mud2::persistence::PersistenceServerPlugin {
            save_path: unique_test_path("world.json"),
        },
        mud2::accounts::AccountsServerPlugin {
            db_path: unique_test_path("accounts.db"),
        },
        mud2::network::TcpServerPlugin {
            bind_addr: None,
            tls_config: None,
        },
        mud2::scripting::GameReplPlugin,
    ));
    app.init_state::<mud2::app::state::ClientAppState>();
    app.add_plugins((
        mud2::world::WorldClientPlugin,
        mud2::player::PlayerClientPlugin,
        mud2::ui::UiPlugin,
        mud2::client_effects::ClientEffectsPlugin,
        mud2::scripting::ScriptingPlugin,
        mud2::diagnostics::DiagnosticsPlugin,
        mud2::crafting::CraftingClientPlugin,
        mud2::log::LogClientPlugin,
        mud2::dialog::DialogServerPlugin,
        mud2::quest::QuestPlugin::default(),
        mud2::network::TcpClientPlugin {
            server_addr: String::new(),
            tls: None,
        },
    ));
    app.add_plugins((
        mud2::app::title_screen::TitleScreenPlugin {
            runtime: AppRuntime::EmbeddedClient,
        },
        mud2::app::asset_sync_screen::AssetSyncScreenPlugin,
        mud2::app::about_screen::AboutScreenPlugin,
        mud2::app::character_select_screen::CharacterSelectScreenPlugin {
            runtime: AppRuntime::EmbeddedClient,
        },
        mud2::app::character_create_screen::CharacterCreateScreenPlugin {
            runtime: AppRuntime::EmbeddedClient,
        },
    ));
    // The map editor rides the embedded app via `embedded_extension`
    // (src/main.rs); its systems belong to the same schedules.
    #[cfg(feature = "editor")]
    app.add_plugins(mud2_editor::editor::EditorPlugin);

    // Building the schedules is the assertion: an unsolvable graph panics
    // inside these updates. Two updates so state-scoped systems initialize
    // too. (Missing render-only resources make systems skip with a warning,
    // which is fine — ordering is what's under test.)
    app.update();
    app.update();
}
