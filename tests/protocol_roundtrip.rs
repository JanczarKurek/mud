//! Wire-format roundtrip coverage for the protocol types. The TCP transport
//! is newline-delimited `serde_json`; the embedded path historically skipped
//! serde entirely, so a non-roundtripping variant (skipped field, unmappable
//! key, lossy repr) would pass every unit test and only fail over the wire.
//!
//! Each enum gets (a) a sample list pushed through serialize → deserialize →
//! re-serialize with `serde_json::Value` equality (the enums don't all derive
//! `PartialEq`), and (b) an exhaustive-match coverage guard: adding a variant
//! breaks compilation here until a sample is added.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use mud2::combat::components::AttackKind;
use mud2::combat::damage_type::DamageType;
use mud2::game::commands::{
    GameCommand, InspectTarget, ItemDestination, ItemReference, ItemSlotRef, MoveDelta,
    RotationDirection, UseTarget,
};
use mud2::game::resources::{
    ClientActiveEffect, ClientCarryWeight, ClientCombatStats, ClientExertion,
    ClientRemotePlayerState, ClientSpaceState, ClientVitalStats, ClientWorldObjectState, GameEvent,
    GameUiEvent, InventoryStackSummary, NpcAwareness, RegenBuffState, SpeechBubbleStyle, VfxAnchor,
};
use mud2::game::trade::{
    ClientTradeView, OfferSource, TradeOfferEntry, TradeOutcome, TradePartnerKind, TradeTarget,
    WareView,
};
use mud2::log::{LogEntry, LogOwner, LogSection, LogState};
use mud2::magic::resources::EffectKind;
use mud2::network::protocol::{AssetEntry, CharacterSummary, ClientMessage, ServerMessage};
use mud2::player::classes::Class;
use mud2::player::components::{
    AttributeKind, AttributeSet, Inventory, InventoryStack, PlayerAppearance, PlayerId,
};
use mud2::player::progression::ExperienceView;
use mud2::player::skills::Skill;
use mud2::world::components::{SpaceId, SpacePosition, TilePosition};
use mud2::world::direction::Direction;
use mud2::world::map_layout::SpaceLightingDef;
use mud2::world::object_definitions::{EquipmentSlot, TextKind};
use serde::de::DeserializeOwned;
use serde::Serialize;

/// serialize → assert single-line (the framing is `\n`-delimited) →
/// deserialize → re-serialize → structural equality.
fn roundtrip<T: Serialize + DeserializeOwned>(value: &T, label: &str) {
    let json =
        serde_json::to_string(value).unwrap_or_else(|e| panic!("{label}: serialize failed: {e}"));
    assert!(
        !json.contains('\n'),
        "{label}: serialized form contains a raw newline — breaks line framing: {json}"
    );
    let back: T = serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("{label}: deserialize failed: {e}\njson: {json}"));
    let value_tree = serde_json::to_value(value).unwrap();
    let back_tree = serde_json::to_value(&back).unwrap();
    assert_eq!(
        value_tree, back_tree,
        "{label}: roundtrip is lossy\njson: {json}"
    );
}

fn tile(x: i32, y: i32, z: i32) -> TilePosition {
    TilePosition { x, y, z }
}

fn space_pos() -> SpacePosition {
    SpacePosition::new(SpaceId(3), tile(4, 5, 0))
}

fn attributes() -> AttributeSet {
    AttributeSet {
        strength: 12,
        agility: 11,
        constitution: 13,
        willpower: 10,
        charisma: 9,
        focus: 14,
    }
}

fn stack() -> InventoryStack {
    let mut properties = HashMap::new();
    // A property value with a newline + unicode: must survive JSON escaping.
    properties.insert("text".to_owned(), "line one\nline twø".to_owned());
    InventoryStack::item("potion", properties, 3)
}

fn inventory() -> Inventory {
    let mut inventory = Inventory::default();
    inventory.backpack_slots[0] = Some(stack());
    inventory.ammo_quantity = 20;
    inventory
}

fn world_object() -> ClientWorldObjectState {
    ClientWorldObjectState {
        object_id: 42,
        definition_id: "iron_chest".to_owned(),
        position: space_pos(),
        tile_position: tile(4, 5, 0),
        vitals: Some(ClientVitalStats {
            health: 7.5,
            max_health: 20.0,
            mana: 0.0,
            max_mana: 0.0,
        }),
        is_container: true,
        is_npc: false,
        is_movable: false,
        is_rotatable: true,
        quantity: 1,
        has_dialog: false,
        facing: Direction::East,
        state: Some("closed".to_owned()),
        is_shopkeeper: false,
        is_hidden: true,
        is_hostile: false,
        is_targeting_local_player: false,
        awareness: Some(NpcAwareness::Searching),
        placement_seq: 9001,
    }
}

fn trade_view() -> ClientTradeView {
    ClientTradeView {
        session_id: 7,
        partner_name: "Merla".to_owned(),
        partner_kind: TradePartnerKind::Shopkeeper,
        our_offers: vec![TradeOfferEntry {
            source: OfferSource::PlayerSlot(ItemSlotRef::Backpack(2)),
            type_id: "potion".to_owned(),
            properties: HashMap::new(),
            quantity: 1,
        }],
        their_offers: vec![TradeOfferEntry {
            source: OfferSource::Stockpile { ware_index: 0 },
            type_id: "sword".to_owned(),
            properties: HashMap::new(),
            quantity: 1,
        }],
        our_ready: true,
        their_ready: false,
        our_confirmed: false,
        their_confirmed: false,
        wares: Some(vec![WareView {
            type_id: "sword".to_owned(),
            display_name: "Iron Sword".to_owned(),
            price_copper: 120,
            stock_remaining: Some(2),
            persuasion_modifier_pct: -12,
        }]),
    }
}

fn log_state() -> LogState {
    let mut subsections = BTreeMap::new();
    subsections.insert(
        "rats".to_owned(),
        LogEntry {
            title: "Rat problem".to_owned(),
            body: "Cellar\nfull of rats.".to_owned(),
            player_notes: "bring cheese".to_owned(),
            owner: LogOwner::Engine,
        },
    );
    let mut sections = BTreeMap::new();
    sections.insert("quests".to_owned(), LogSection { subsections });
    LogState { sections }
}

fn game_command_samples() -> Vec<GameCommand> {
    vec![
        GameCommand::MovePlayer {
            delta: MoveDelta { x: -1, y: 1 },
            climb: true,
        },
        GameCommand::JumpTo {
            target_tile: tile(8, 9, 1),
        },
        GameCommand::RotateObject {
            object_id: 4,
            rotation: RotationDirection::CounterClockwise,
        },
        GameCommand::SetCombatTarget {
            target_object_id: Some(11),
        },
        GameCommand::OpenContainer { object_id: 5 },
        GameCommand::CloseContainer { object_id: 5 },
        GameCommand::InteractWithObject {
            object_id: 6,
            verb: "open".to_owned(),
        },
        GameCommand::HideObject { object_id: 6 },
        GameCommand::ApplyToolInteraction {
            target_object_id: 6,
            verb: "pry".to_owned(),
        },
        GameCommand::Inspect {
            target: InspectTarget::SlotItem(ItemSlotRef::PouchInBackpack {
                backpack_slot: 1,
                sub_slot: 0,
            }),
        },
        GameCommand::UseItem {
            source: ItemReference::Slot(ItemSlotRef::Equipment(EquipmentSlot::Weapon)),
        },
        GameCommand::UseItemOn {
            source: ItemReference::Slot(ItemSlotRef::Backpack(0)),
            target: UseTarget::ItemSlot(ItemSlotRef::Container {
                object_id: 5,
                slot_index: 2,
            }),
        },
        GameCommand::CastSpellAt {
            source: ItemReference::Slot(ItemSlotRef::Backpack(0)),
            spell_id: "firebolt".to_owned(),
            target_object_id: 11,
        },
        GameCommand::CastSpellAtTile {
            source: ItemReference::Slot(ItemSlotRef::Backpack(0)),
            spell_id: "firewall".to_owned(),
            target_tile: tile(2, 3, 0),
        },
        GameCommand::CastSpellAtItem {
            source: ItemReference::Slot(ItemSlotRef::Backpack(0)),
            spell_id: "enchant_weapon".to_owned(),
            target: ItemSlotRef::Equipment(EquipmentSlot::Weapon),
        },
        GameCommand::MoveItem {
            source: ItemReference::WorldObject(42),
            destination: ItemDestination::Slot(ItemSlotRef::Backpack(3)),
        },
        GameCommand::TakeFromStack {
            source: ItemReference::Slot(ItemSlotRef::Backpack(0)),
            amount: 2,
            destination: ItemDestination::WorldTile(tile(1, 1, 0)),
        },
        GameCommand::AdminSpawn {
            type_id: "rat".to_owned(),
            tile_position: tile(10, 10, 0),
        },
        GameCommand::AdminTeleport {
            space_id: Some(SpaceId(2)),
            tile_position: tile(0, 0, 0),
        },
        GameCommand::AdminDespawn { object_id: 42 },
        GameCommand::AdminSetVitals {
            health: Some(10.0),
            mana: None,
        },
        GameCommand::AdminSetObjectState {
            object_id: 5,
            state: "open".to_owned(),
        },
        GameCommand::TalkToNpc { npc_object_id: 12 },
        GameCommand::DialogAdvance { session_id: 1 },
        GameCommand::DialogChoose {
            session_id: 1,
            option_idx: 2,
        },
        GameCommand::DialogEnd { session_id: 1 },
        GameCommand::GiveItem {
            type_id: "potion".to_owned(),
            count: 3,
        },
        GameCommand::TakeItem {
            type_id: "potion".to_owned(),
            count: 1,
        },
        GameCommand::EditorSetFloorTile {
            space_id: SpaceId(1),
            z: 0,
            x: 3,
            y: 4,
            floor_type: Some("grass".to_owned()),
        },
        GameCommand::SetHome,
        GameCommand::SetSneaking { sneaking: true },
        GameCommand::SetAware { aware: false },
        GameCommand::SetAutoRetaliate {
            auto_retaliate: true,
        },
        GameCommand::AcknowledgeDeath,
        GameCommand::InitiateTrade {
            target: TradeTarget::Shopkeeper { object_id: 12 },
        },
        GameCommand::OfferTradeItem {
            session_id: 7,
            source: ItemSlotRef::Backpack(0),
            quantity: 1,
        },
        GameCommand::WithdrawTradeItem {
            session_id: 7,
            offer_index: 0,
        },
        GameCommand::ToggleTradeReady { session_id: 7 },
        GameCommand::ConfirmTrade { session_id: 7 },
        GameCommand::CancelTrade { session_id: 7 },
        GameCommand::BrowseShopBuy {
            session_id: 7,
            ware_index: 0,
            quantity: 2,
        },
        GameCommand::StashMutate {
            key: "quest:rats".to_owned(),
            value: Some(serde_json::json!({"stage": 2, "done": false})),
        },
        GameCommand::LearnRecipe {
            recipe_id: "bread".to_owned(),
        },
        GameCommand::CraftItem {
            recipe_id: "bread".to_owned(),
        },
        GameCommand::Say {
            text: "hello\nworld".to_owned(),
        },
        GameCommand::UpsertLogEntry {
            section: "notes".to_owned(),
            subsection: "misc".to_owned(),
            title: "Note".to_owned(),
            body: "body".to_owned(),
            owner: LogOwner::Player,
        },
        GameCommand::DeleteLogEntry {
            section: "notes".to_owned(),
            subsection: "misc".to_owned(),
        },
        GameCommand::SetQuestPlayerNotes {
            quest_name: "rats".to_owned(),
            text: "bring cheese".to_owned(),
        },
        GameCommand::AllocateSkillPoint {
            skill: Skill::Stealth,
            ranks: 2,
        },
        GameCommand::AllocateAbilityBump {
            attribute: AttributeKind::Agility,
        },
        GameCommand::AdminGrantXp { amount: 500 },
        GameCommand::AdminSetLevel { level: 4 },
        GameCommand::AdminGrantSkillPoints { amount: 3 },
        GameCommand::AdminSetSkillRank {
            skill: Skill::Perception,
            rank: 5,
        },
        GameCommand::AdminSetAttribute {
            kind: AttributeKind::Focus,
            value: 18,
        },
        GameCommand::AdminSetClass {
            class: Class::Wizard,
        },
        GameCommand::AdminFullHeal,
        GameCommand::AdminToggleGodMode,
        GameCommand::AdminToggleNoclip,
        GameCommand::ReadBook {
            source: ItemReference::WorldObject(9),
        },
        GameCommand::WriteBook {
            source: ItemReference::WorldObject(9),
            title: "Diary".to_owned(),
            text: "day 1\nday 2".to_owned(),
        },
        GameCommand::Engrave {
            source: ItemReference::Slot(ItemSlotRef::Backpack(0)),
            inscription: "for Merla".to_owned(),
        },
        GameCommand::AdminExec {
            code: "def f():\n    return 1".to_owned(),
        },
        GameCommand::AdminReplReset,
        GameCommand::AdminSetAccountAdmin {
            username: "merla".to_owned(),
            admin: true,
        },
    ]
}

/// Coverage guard: adding a `GameCommand` variant breaks this match — add a
/// sample to `game_command_samples` when it does.
#[allow(dead_code)]
fn game_command_coverage(command: &GameCommand) {
    match command {
        GameCommand::MovePlayer { .. }
        | GameCommand::JumpTo { .. }
        | GameCommand::RotateObject { .. }
        | GameCommand::SetCombatTarget { .. }
        | GameCommand::OpenContainer { .. }
        | GameCommand::CloseContainer { .. }
        | GameCommand::InteractWithObject { .. }
        | GameCommand::HideObject { .. }
        | GameCommand::ApplyToolInteraction { .. }
        | GameCommand::Inspect { .. }
        | GameCommand::UseItem { .. }
        | GameCommand::UseItemOn { .. }
        | GameCommand::CastSpellAt { .. }
        | GameCommand::CastSpellAtTile { .. }
        | GameCommand::CastSpellAtItem { .. }
        | GameCommand::MoveItem { .. }
        | GameCommand::TakeFromStack { .. }
        | GameCommand::AdminSpawn { .. }
        | GameCommand::AdminTeleport { .. }
        | GameCommand::AdminDespawn { .. }
        | GameCommand::AdminSetVitals { .. }
        | GameCommand::AdminSetObjectState { .. }
        | GameCommand::TalkToNpc { .. }
        | GameCommand::DialogAdvance { .. }
        | GameCommand::DialogChoose { .. }
        | GameCommand::DialogEnd { .. }
        | GameCommand::GiveItem { .. }
        | GameCommand::TakeItem { .. }
        | GameCommand::EditorSetFloorTile { .. }
        | GameCommand::SetHome
        | GameCommand::SetSneaking { .. }
        | GameCommand::SetAware { .. }
        | GameCommand::SetAutoRetaliate { .. }
        | GameCommand::AcknowledgeDeath
        | GameCommand::InitiateTrade { .. }
        | GameCommand::OfferTradeItem { .. }
        | GameCommand::WithdrawTradeItem { .. }
        | GameCommand::ToggleTradeReady { .. }
        | GameCommand::ConfirmTrade { .. }
        | GameCommand::CancelTrade { .. }
        | GameCommand::BrowseShopBuy { .. }
        | GameCommand::StashMutate { .. }
        | GameCommand::LearnRecipe { .. }
        | GameCommand::CraftItem { .. }
        | GameCommand::Say { .. }
        | GameCommand::UpsertLogEntry { .. }
        | GameCommand::DeleteLogEntry { .. }
        | GameCommand::SetQuestPlayerNotes { .. }
        | GameCommand::AllocateSkillPoint { .. }
        | GameCommand::AllocateAbilityBump { .. }
        | GameCommand::AdminGrantXp { .. }
        | GameCommand::AdminSetLevel { .. }
        | GameCommand::AdminGrantSkillPoints { .. }
        | GameCommand::AdminSetSkillRank { .. }
        | GameCommand::AdminSetAttribute { .. }
        | GameCommand::AdminSetClass { .. }
        | GameCommand::AdminFullHeal
        | GameCommand::AdminToggleGodMode
        | GameCommand::AdminToggleNoclip
        | GameCommand::ReadBook { .. }
        | GameCommand::WriteBook { .. }
        | GameCommand::Engrave { .. }
        | GameCommand::AdminExec { .. }
        | GameCommand::AdminReplReset
        | GameCommand::AdminSetAccountAdmin { .. } => {}
    }
}

fn game_event_samples() -> Vec<GameEvent> {
    let mut discovered = HashMap::new();
    discovered.insert(SpaceId(1), vec![(0, 0), (1, 0)]);
    vec![
        GameEvent::LocalPlayerIdentified {
            player_id: PlayerId(1),
            object_id: 100,
        },
        GameEvent::InventoryChanged {
            inventory: inventory(),
        },
        GameEvent::ChatLogChanged {
            lines: vec!["[Merla]: hi".to_owned()],
        },
        GameEvent::PlayerPositionChanged {
            position: space_pos(),
            tile_position: tile(4, 5, 0),
            facing: Direction::North,
        },
        GameEvent::CurrentSpaceChanged {
            space: ClientSpaceState {
                space_id: SpaceId(3),
                authored_id: "overworld".to_owned(),
                width: 64,
                height: 64,
                fill_floor_type: "grass".to_owned(),
                lighting: SpaceLightingDef::default(),
            },
        },
        GameEvent::PlayerVitalsChanged {
            vitals: ClientVitalStats {
                health: 12.5,
                max_health: 20.0,
                mana: 3.0,
                max_mana: 10.0,
            },
        },
        GameEvent::PlayerRegenBuffChanged {
            buff: Some(RegenBuffState {
                multiplier: 1.5,
                remaining_seconds: 30.0,
            }),
        },
        GameEvent::PlayerEffectsChanged {
            effects: vec![ClientActiveEffect {
                kind: EffectKind::Haste,
                magnitude: 0.75,
                remaining_seconds: 12.0,
                secondary_magnitude: Some(0.5),
            }],
        },
        GameEvent::PlayerSneakingChanged { sneaking: true },
        GameEvent::PlayerAwareChanged { aware: true },
        GameEvent::PlayerAutoRetaliateChanged {
            auto_retaliate: false,
        },
        GameEvent::PlayerExertionChanged {
            exertion: ClientExertion {
                current: 4.0,
                max: 10.0,
            },
        },
        GameEvent::PlayerStorageChanged { storage_slots: 16 },
        GameEvent::PlayerCarryWeightChanged {
            carry: ClientCarryWeight {
                current_kg: 12.35,
                soft_cap_kg: 30.0,
                hard_cap_kg: 45.0,
                encumbered: false,
            },
        },
        GameEvent::CombatTargetChanged {
            target_object_id: Some(11),
        },
        GameEvent::ContainerChanged {
            object_id: 5,
            slots: vec![Some(stack()), None],
        },
        GameEvent::ContainerRemoved { object_id: 5 },
        GameEvent::WorldObjectUpserted {
            object: world_object(),
        },
        GameEvent::WorldObjectRemoved { object_id: 42 },
        GameEvent::RemotePlayerUpserted {
            player: ClientRemotePlayerState {
                player_id: PlayerId(2),
                object_id: 101,
                position: space_pos(),
                tile_position: tile(6, 6, 0),
                vitals: ClientVitalStats {
                    health: 20.0,
                    max_health: 20.0,
                    mana: 0.0,
                    max_mana: 0.0,
                },
                facing: Direction::West,
            },
        },
        GameEvent::RemotePlayerRemoved {
            player_id: PlayerId(2),
        },
        GameEvent::FloorMapReplaced {
            space_id: SpaceId(3),
            z: 0,
            width: 2,
            height: 1,
            tiles: vec![Some("grass".to_owned()), None],
        },
        GameEvent::FloorTileSet {
            space_id: SpaceId(3),
            z: 0,
            x: 1,
            y: 0,
            floor_type: None,
        },
        GameEvent::WorldTimeChanged { time_of_day: 0.5 },
        GameEvent::PlayerExperienceChanged {
            experience: ExperienceView {
                current_xp: 900,
                level: 2,
                xp_into_level: 400,
                xp_for_next: Some(1000),
            },
        },
        GameEvent::PlayerClassChanged {
            class: Class::Cleric,
        },
        GameEvent::PlayerAttributesChanged {
            attributes: attributes(),
        },
        GameEvent::PlayerCombatStatsChanged {
            stats: ClientCombatStats {
                attack_kind: AttackKind::Ranged { range_tiles: 5 },
                damage_type: DamageType::Pierce,
                damage_min: 2,
                damage_max: 7,
                attack_bonus: 3,
                dodge_dc: 12,
                armor: 4,
                block: 2,
                block_chance_pct: 30,
                has_shield: true,
            },
        },
        GameEvent::TradeStateChanged {
            state: Some(trade_view()),
        },
        GameEvent::LearnedRecipesChanged {
            recipes: BTreeSet::from(["bread".to_owned(), "stew".to_owned()]),
        },
        GameEvent::LogStateChanged { state: log_state() },
        GameEvent::SkillSheetChanged {
            ranks: [0, 1, 2, 3, 4, 5, 0, 1, 2, 3],
            available_points: 2,
            available_ability_bumps: 1,
        },
        GameEvent::DiscoveredTilesReplaced { tiles: discovered },
        GameEvent::TilesDiscovered {
            space_id: SpaceId(1),
            tiles: vec![(2, 2)],
        },
    ]
}

/// Coverage guard: adding a `GameEvent` variant breaks this match — add a
/// sample to `game_event_samples` when it does.
#[allow(dead_code)]
fn game_event_coverage(event: &GameEvent) {
    match event {
        GameEvent::LocalPlayerIdentified { .. }
        | GameEvent::InventoryChanged { .. }
        | GameEvent::ChatLogChanged { .. }
        | GameEvent::PlayerPositionChanged { .. }
        | GameEvent::CurrentSpaceChanged { .. }
        | GameEvent::PlayerVitalsChanged { .. }
        | GameEvent::PlayerRegenBuffChanged { .. }
        | GameEvent::PlayerEffectsChanged { .. }
        | GameEvent::PlayerSneakingChanged { .. }
        | GameEvent::PlayerAwareChanged { .. }
        | GameEvent::PlayerAutoRetaliateChanged { .. }
        | GameEvent::PlayerExertionChanged { .. }
        | GameEvent::PlayerStorageChanged { .. }
        | GameEvent::PlayerCarryWeightChanged { .. }
        | GameEvent::CombatTargetChanged { .. }
        | GameEvent::ContainerChanged { .. }
        | GameEvent::ContainerRemoved { .. }
        | GameEvent::WorldObjectUpserted { .. }
        | GameEvent::WorldObjectRemoved { .. }
        | GameEvent::RemotePlayerUpserted { .. }
        | GameEvent::RemotePlayerRemoved { .. }
        | GameEvent::FloorMapReplaced { .. }
        | GameEvent::FloorTileSet { .. }
        | GameEvent::WorldTimeChanged { .. }
        | GameEvent::PlayerExperienceChanged { .. }
        | GameEvent::PlayerClassChanged { .. }
        | GameEvent::PlayerAttributesChanged { .. }
        | GameEvent::PlayerCombatStatsChanged { .. }
        | GameEvent::TradeStateChanged { .. }
        | GameEvent::LearnedRecipesChanged { .. }
        | GameEvent::LogStateChanged { .. }
        | GameEvent::SkillSheetChanged { .. }
        | GameEvent::DiscoveredTilesReplaced { .. }
        | GameEvent::TilesDiscovered { .. } => {}
    }
}

fn game_ui_event_samples() -> Vec<GameUiEvent> {
    vec![
        GameUiEvent::OpenContainer { object_id: 5 },
        GameUiEvent::ProjectileFired {
            from_tile: tile(0, 0, 0),
            to_tile: tile(3, 3, 0),
            sprite_definition_id: "arrow".to_owned(),
            duration_seconds: 0.4,
            target_object_id: Some(11),
        },
        GameUiEvent::DialogLine {
            session_id: 1,
            speaker: Some("Merla".to_owned()),
            text: "Hello,\ntraveler.".to_owned(),
        },
        GameUiEvent::DialogOptions {
            session_id: 1,
            options: vec!["Buy".to_owned(), "Leave".to_owned()],
        },
        GameUiEvent::DialogClose { session_id: 1 },
        GameUiEvent::LevelUpToast { new_level: 2 },
        GameUiEvent::DeathSummary {
            items_dropped: vec![InventoryStackSummary {
                type_id: "potion".to_owned(),
                display_name: "Potion".to_owned(),
                quantity: 3,
            }],
            xp_lost: 250,
        },
        GameUiEvent::OpenTradePanel { session_id: 7 },
        GameUiEvent::CloseTradePanel {
            session_id: 7,
            outcome: TradeOutcome::PartnerDisconnected,
        },
        GameUiEvent::VfxSpawn {
            definition_id: "impact_fire".to_owned(),
            anchor: VfxAnchor::Tile {
                space_id: SpaceId(3),
                tile: tile(4, 5, 0),
            },
        },
        GameUiEvent::VfxSpawn {
            definition_id: "buff_glow".to_owned(),
            anchor: VfxAnchor::FollowObject {
                object_id: 42,
                offset_pixels: [0.0, -8.0],
            },
        },
        GameUiEvent::RecipeLearnedToast {
            recipe_id: "bread".to_owned(),
            recipe_name: "Bread".to_owned(),
        },
        GameUiEvent::OpenRecipeBook {
            filter_station: Some("oven".to_owned()),
        },
        GameUiEvent::OpenSkillsPanel,
        GameUiEvent::SkillPointsToast { amount: 2 },
        GameUiEvent::AbilityBumpAvailable { available: 1 },
        GameUiEvent::AttackDodged {
            attacker_object_id: 11,
            target_object_id: 100,
        },
        GameUiEvent::Spotted { npc_object_id: 11 },
        GameUiEvent::AttackBlocked {
            attacker_object_id: 11,
            target_object_id: 100,
            amount: 4,
        },
        GameUiEvent::AttackCrit {
            attacker_object_id: 100,
            target_object_id: 11,
        },
        GameUiEvent::OpenBookPanel {
            source: ItemReference::WorldObject(9),
            kind: TextKind::Tombstone,
            title: "R.I.P.".to_owned(),
            text: "Here lies\na hero.".to_owned(),
            author_name: None,
            can_edit: false,
        },
        GameUiEvent::SpeechBubble {
            speaker_object_id: 11,
            text: "grr".to_owned(),
            style: SpeechBubbleStyle::Bark,
        },
        GameUiEvent::ReplOutput {
            lines: vec!["2".to_owned()],
            error: Some("Traceback (most recent call last):\nboom".to_owned()),
            incomplete: false,
        },
    ]
}

/// Coverage guard: adding a `GameUiEvent` variant breaks this match — add a
/// sample to `game_ui_event_samples` when it does.
#[allow(dead_code)]
fn game_ui_event_coverage(event: &GameUiEvent) {
    match event {
        GameUiEvent::OpenContainer { .. }
        | GameUiEvent::ProjectileFired { .. }
        | GameUiEvent::DialogLine { .. }
        | GameUiEvent::DialogOptions { .. }
        | GameUiEvent::DialogClose { .. }
        | GameUiEvent::LevelUpToast { .. }
        | GameUiEvent::DeathSummary { .. }
        | GameUiEvent::OpenTradePanel { .. }
        | GameUiEvent::CloseTradePanel { .. }
        | GameUiEvent::VfxSpawn { .. }
        | GameUiEvent::RecipeLearnedToast { .. }
        | GameUiEvent::OpenRecipeBook { .. }
        | GameUiEvent::OpenSkillsPanel
        | GameUiEvent::SkillPointsToast { .. }
        | GameUiEvent::AbilityBumpAvailable { .. }
        | GameUiEvent::AttackDodged { .. }
        | GameUiEvent::Spotted { .. }
        | GameUiEvent::AttackBlocked { .. }
        | GameUiEvent::AttackCrit { .. }
        | GameUiEvent::OpenBookPanel { .. }
        | GameUiEvent::SpeechBubble { .. }
        | GameUiEvent::ReplOutput { .. } => {}
    }
}

fn client_message_samples() -> Vec<ClientMessage> {
    vec![
        ClientMessage::Command(GameCommand::SetHome),
        ClientMessage::AssetRequest(vec!["sprites/potion.png".to_owned()]),
        ClientMessage::SyncComplete,
        ClientMessage::Login {
            username: "merla".to_owned(),
            password: "secret123".to_owned(),
        },
        ClientMessage::Register {
            username: "merla".to_owned(),
            password: "secret123".to_owned(),
        },
        ClientMessage::ListCharacters,
        ClientMessage::CreateCharacter {
            name: "Grabby".to_owned(),
            class: Class::Vagabond,
            attributes: attributes(),
            appearance: PlayerAppearance::default(),
        },
        ClientMessage::SelectCharacter {
            character_id: 3,
            start_map: Some("overworld".to_owned()),
        },
        ClientMessage::DeleteCharacter { character_id: 3 },
        ClientMessage::Pong { nonce: 12345 },
    ]
}

/// Coverage guard for `ClientMessage`.
#[allow(dead_code)]
fn client_message_coverage(message: &ClientMessage) {
    match message {
        ClientMessage::Command(_)
        | ClientMessage::AssetRequest(_)
        | ClientMessage::SyncComplete
        | ClientMessage::Login { .. }
        | ClientMessage::Register { .. }
        | ClientMessage::ListCharacters
        | ClientMessage::CreateCharacter { .. }
        | ClientMessage::SelectCharacter { .. }
        | ClientMessage::DeleteCharacter { .. }
        | ClientMessage::Pong { .. } => {}
    }
}

fn server_message_samples() -> Vec<ServerMessage> {
    vec![
        ServerMessage::Events(game_event_samples()),
        ServerMessage::UiEvents(game_ui_event_samples()),
        ServerMessage::AssetManifest(vec![AssetEntry {
            path: "sprites/potion.png".to_owned(),
            hash: "abc123".to_owned(),
        }]),
        ServerMessage::AssetData {
            path: "sprites/potion.png".to_owned(),
            data: "aGVsbG8=".to_owned(),
        },
        ServerMessage::AuthResult {
            ok: false,
            reason: Some("wrong password".to_owned()),
        },
        ServerMessage::CharacterList(vec![CharacterSummary {
            character_id: 3,
            name: "Grabby".to_owned(),
            class: Class::Fighter,
            level: 2,
        }]),
        ServerMessage::CharacterCreateResult {
            ok: true,
            character_id: Some(3),
            reason: None,
        },
        ServerMessage::CharacterSelected { character_id: 3 },
        ServerMessage::Ping { nonce: 12345 },
    ]
}

/// Coverage guard for `ServerMessage`.
#[allow(dead_code)]
fn server_message_coverage(message: &ServerMessage) {
    match message {
        ServerMessage::Events(_)
        | ServerMessage::UiEvents(_)
        | ServerMessage::AssetManifest(_)
        | ServerMessage::AssetData { .. }
        | ServerMessage::AuthResult { .. }
        | ServerMessage::CharacterList(_)
        | ServerMessage::CharacterCreateResult { .. }
        | ServerMessage::CharacterSelected { .. }
        | ServerMessage::Ping { .. } => {}
    }
}

#[test]
fn game_commands_roundtrip() {
    for (i, command) in game_command_samples().iter().enumerate() {
        roundtrip(command, &format!("GameCommand[{i}] {command:?}"));
    }
}

#[test]
fn game_events_roundtrip() {
    for (i, event) in game_event_samples().iter().enumerate() {
        roundtrip(event, &format!("GameEvent[{i}] {event:?}"));
    }
}

#[test]
fn game_ui_events_roundtrip() {
    for (i, event) in game_ui_event_samples().iter().enumerate() {
        roundtrip(event, &format!("GameUiEvent[{i}] {event:?}"));
    }
}

#[test]
fn client_messages_roundtrip() {
    for (i, message) in client_message_samples().iter().enumerate() {
        roundtrip(message, &format!("ClientMessage[{i}] {message:?}"));
    }
}

#[test]
fn server_messages_roundtrip() {
    for (i, message) in server_message_samples().iter().enumerate() {
        roundtrip(message, &format!("ServerMessage[{i}] {message:?}"));
    }
}
