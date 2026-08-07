//! Shared harness for in-crate unit tests.
//!
//! Compiled only under `cfg(test)` (see `lib.rs`). Individual test modules
//! layer their file-specific extras (unique bundles, extra resources, the
//! systems under test) on top of these builders instead of maintaining
//! drifted copies of the same setup code.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::combat::components::{AttackProfile, CombatLeash};
use crate::combat::CombatPlugin;
use crate::game::commands::{GameCommand, MoveDelta};
use crate::game::resources::{PendingGameCommands, PendingGameEvents, PendingGameUiEvents};
use crate::game::GameServerPlugin;
use crate::magic::MagicServerPlugin;
use crate::npc::NpcPlugin;
use crate::persistence::PersistenceServerPlugin;
use crate::player::components::{
    BaseStats, ChatLog, DefenseStats, DerivedStats, Inventory, MovementCooldown, Player, PlayerId,
    PlayerIdentity, VitalStats, WeaponDamage,
};
use crate::player::skills::SkillSheet;
use crate::player::PlayerServerPlugin;
use crate::quest::QuestPlugin;
use crate::world::components::{Collider, OverworldObject, SpaceId, SpaceResident, TilePosition};
use crate::world::object_registry::ObjectRegistry;
use crate::world::{WorldConfig, WorldServerPlugin};

/// Builder for a server-side test `App`: `MinimalPlugins` plus the standard
/// server plugin set (`GameServerPlugin`, `WorldServerPlugin`,
/// `PlayerServerPlugin`, `MagicServerPlugin`), with opt-in extras. `build()`
/// runs one `app.update()` so startup systems have executed.
#[derive(Default)]
pub struct TestServerApp {
    npc_plugin: bool,
    combat_plugin: bool,
    quest_plugin: bool,
    persistence_save_path: Option<PathBuf>,
    tcp_server_state: bool,
    character_var_stores: bool,
}

impl TestServerApp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_npc_plugin(mut self) -> Self {
        self.npc_plugin = true;
        self
    }

    pub fn with_combat_plugin(mut self) -> Self {
        self.combat_plugin = true;
        self
    }

    pub fn with_quest_plugin(mut self) -> Self {
        self.quest_plugin = true;
        self
    }

    pub fn with_persistence(mut self, save_path: &Path) -> Self {
        self.persistence_save_path = Some(save_path.to_path_buf());
        self
    }

    pub fn with_tcp_server_state(mut self) -> Self {
        self.tcp_server_state = true;
        self
    }

    /// `CharacterVarStores` normally comes from `DialogServerPlugin`, but that
    /// plugin pulls in YarnSpinner which needs `AssetPlugin`. Systems that only
    /// require the resource to exist can inject the bare default instead.
    pub fn with_character_var_stores(mut self) -> Self {
        self.character_var_stores = true;
        self
    }

    pub fn build(self) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        if self.tcp_server_state {
            app.insert_resource(crate::network::resources::TcpServerState::default());
        }
        if self.character_var_stores {
            app.init_resource::<crate::dialog::resources::CharacterVarStores>();
        }
        app.add_plugins((GameServerPlugin, WorldServerPlugin));
        if self.npc_plugin {
            app.add_plugins(NpcPlugin);
        }
        app.add_plugins(PlayerServerPlugin);
        if self.combat_plugin {
            app.add_plugins(CombatPlugin);
        }
        app.add_plugins(MagicServerPlugin);
        if self.quest_plugin {
            app.add_plugins(QuestPlugin::default());
        }
        if let Some(save_path) = self.persistence_save_path {
            app.add_plugins(PersistenceServerPlugin { save_path });
        }
        app.update();
        app
    }
}

/// Bare `App` with the pending command/event queues that server systems drain,
/// for tests that register a single system directly instead of full plugins.
pub fn minimal_command_app() -> App {
    let mut app = App::new();
    app.init_resource::<PendingGameCommands>()
        .init_resource::<PendingGameEvents>()
        .init_resource::<PendingGameUiEvents>();
    app
}

/// Spawn the standard authoritative server-side player bundle used by
/// command-loop tests, in the current space at `(x, y)` on the ground floor.
/// Does *not* include `MagicEffects` — see `spawn_server_player_with_magic`.
pub fn spawn_server_player(app: &mut App, player_id: u64, x: i32, y: i32) -> Entity {
    spawn_server_player_impl(app, player_id, x, y, false)
}

/// `spawn_server_player` plus a default `MagicEffects` component.
pub fn spawn_server_player_with_magic(app: &mut App, player_id: u64, x: i32, y: i32) -> Entity {
    spawn_server_player_impl(app, player_id, x, y, true)
}

fn spawn_server_player_impl(
    app: &mut App,
    player_id: u64,
    x: i32,
    y: i32,
    magic_effects: bool,
) -> Entity {
    let base_stats = BaseStats::default();
    let derived_stats = DerivedStats::from_base(&base_stats);
    let max_health = derived_stats.max_health as f32;
    let max_mana = derived_stats.max_mana as f32;
    let current_space_id = app.world().resource::<WorldConfig>().current_space_id;
    let object_id = app
        .world_mut()
        .resource_mut::<ObjectRegistry>()
        .allocate_runtime_id("player");
    let mut entity = app.world_mut().spawn((
        Player,
        PlayerIdentity::new(PlayerId(player_id)),
        Inventory::default(),
        ChatLog::default(),
        (base_stats, derived_stats, SkillSheet::default()),
        VitalStats::full(max_health, max_mana),
        MovementCooldown::default(),
        (
            AttackProfile::melee(),
            WeaponDamage::default(),
            DefenseStats::default(),
        ),
        CombatLeash {
            max_distance_tiles: 6,
        },
        Collider,
        OverworldObject {
            object_id,
            definition_id: "player".to_owned(),
            placement_seq: 0,
        },
        SpaceResident {
            space_id: current_space_id,
        },
        TilePosition::ground(x, y),
    ));
    if magic_effects {
        entity.insert(crate::magic::effects::MagicEffects::default());
    }
    entity.id()
}

/// Queue a `GameCommand` as if it came from `player_id`.
pub fn push_player_command(app: &mut App, player_id: u64, command: GameCommand) {
    app.world_mut()
        .resource_mut::<PendingGameCommands>()
        .push_for_player(PlayerId(player_id), command);
}

/// Queue a non-climbing `MovePlayer` step for `player_id`.
pub fn push_move(app: &mut App, player_id: u64, dx: i32, dy: i32) {
    push_player_command(
        app,
        player_id,
        GameCommand::MovePlayer {
            delta: MoveDelta { x: dx, y: dy },
            climb: false,
        },
    );
}

/// The `WorldConfig` presentation-side tests assume (32×24 grass map).
pub fn test_world_config() -> WorldConfig {
    WorldConfig {
        current_space_id: SpaceId(1),
        map_width: 32,
        map_height: 24,
        tile_size: 48.0,
        fill_floor_type: "grass".to_owned(),
    }
}
