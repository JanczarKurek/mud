//! Trading & shopping core types and server-side logic.
//!
//! Two roles share this code path:
//! - Player-to-player trading (Phase A)
//! - Shopkeeper trading via a `Stockpile` (Phase B+)
//!
//! Items remain in their owners' inventories until both sides confirm; offers
//! carry only the *source slot* and the projected `(type_id, qty)`. On commit,
//! the server validates each source still resolves and atomically transfers
//! goods. Trades are ephemeral — they live in the `ActiveTrades` resource only
//! and are aborted on disconnect / out-of-range.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "server-sim")]
use crate::game::commands::GameCommand;
use crate::game::commands::ItemSlotRef;
// Ungated: `offer_credit_copper` is shared by the (gated) commit path and the
// projection that previews the same number for thin clients.
use crate::game::currency::{
    COPPER_PER_GOLD, COPPER_PER_SILVER, COPPER_TYPE_ID, GOLD_TYPE_ID, SILVER_TYPE_ID,
};
#[cfg(feature = "server-sim")]
use crate::game::helpers::PlayerActorQuery;
#[cfg(feature = "server-sim")]
use crate::game::resources::{
    GameUiEvent, InventoryState, PendingGameCommands, PendingGameUiEvents,
};
#[cfg(feature = "server-sim")]
use crate::game::shop::{Shopkeeper, StockEntry, StockMode, Stockpile};
use crate::player::components::PlayerId;
#[cfg(feature = "server-sim")]
use crate::player::components::{InventoryStack, MaxCarryWeight, Player, PlayerIdentity};
#[cfg(feature = "server-sim")]
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::map_layout::ObjectProperties;
// Ungated for the same reason as the currency constants above.
use crate::world::object_definitions::OverworldObjectDefinitions;

pub type TradeSessionId = u64;

/// What the initiating player picked as the trade target. Resolved into a
/// `TradeParticipants` by the server — for `Player`, the `object_id` is mapped
/// to a `PlayerId`; for `Shopkeeper`, the npc must carry a `Shopkeeper`
/// component (Phase B).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TradeTarget {
    Player { object_id: u64 },
    Shopkeeper { object_id: u64 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TradeOutcome {
    Completed,
    Cancelled,
    PartnerDisconnected,
    OutOfRange,
}

/// Origin of an offered item — describes *where* the item is so the server can
/// re-validate at commit time.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum OfferSource {
    /// An item from one of the acting player's personal slots
    /// (Backpack/Equipment/PouchInBackpack).
    PlayerSlot(ItemSlotRef),
    /// A ware drawn from a shopkeeper's `Stockpile`. `ware_index` is the
    /// position into `Stockpile.wares` at session-open time. Used only on the
    /// shopkeeper's "us" side of a `PlayerToShop` session.
    Stockpile { ware_index: usize },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TradeOfferEntry {
    pub source: OfferSource,
    pub type_id: String,
    pub properties: ObjectProperties,
    pub quantity: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TradePartnerKind {
    Player,
    Shopkeeper,
}

/// The local player's view of an active trade. Folded into
/// `ClientGameState.current_trade` by the projection. The "us" / "them"
/// partition is computed per-recipient at projection time.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClientTradeView {
    pub session_id: TradeSessionId,
    pub partner_name: String,
    pub partner_kind: TradePartnerKind,
    pub our_offers: Vec<TradeOfferEntry>,
    pub their_offers: Vec<TradeOfferEntry>,
    pub our_ready: bool,
    pub their_ready: bool,
    pub our_confirmed: bool,
    pub their_confirmed: bool,
    /// `Some` when the partner is a shopkeeper: the wares list to render in
    /// a "Browse Wares" subpanel. `None` for player-to-player trades.
    #[serde(default)]
    pub wares: Option<Vec<WareView>>,
    /// What the merchant will credit for everything currently in our column,
    /// in copper, Persuasion included. Always 0 for player-to-player trades.
    ///
    /// Computed server-side by the same function the commit path uses, so the
    /// number on screen is the number that gets paid — money is never
    /// recomputed client-side.
    #[serde(default)]
    pub sale_credit_copper: u32,
    /// What the merchant is asking for everything currently in their column,
    /// in copper, Persuasion included. Always 0 for player-to-player trades.
    ///
    /// Same guarantee as `sale_credit_copper`: computed by the pricing
    /// functions the commit path uses, never recomputed client-side.
    #[serde(default)]
    pub total_owed_copper: u32,
}

/// Per-ware projection used by the trade panel's Browse Wares list.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WareView {
    pub type_id: String,
    pub display_name: String,
    pub price_copper: u32,
    /// `None` for infinite stock; `Some(n)` for finite remaining.
    pub stock_remaining: Option<u32>,
    /// Signed percent change vs the stockpile's base price for the local
    /// viewer (e.g. `-12` for "12% off"). `0` when no Persuasion modifier
    /// is in effect. The server projects modified `price_copper` into the
    /// view; this field is purely for UI annotation.
    #[serde(default)]
    pub persuasion_modifier_pct: i8,
}

/// Which side of a vendor transaction the player is on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TradeSide {
    /// Player pays the merchant — discount on prices.
    PlayerBuys,
    /// Player sells to the merchant — premium on offered prices.
    PlayerSells,
}

/// Maximum absolute Persuasion adjustment (per `docs/skills_locks_social_plan.md`
/// locked-in decisions: ±20% cap, 2% per rank).
pub const PERSUASION_MAX_PCT: i32 = 20;
pub const PERSUASION_PCT_PER_RANK: i32 = 2;

/// Compute the modified price the merchant offers / accepts given the
/// player's Persuasion ranks. Buyer-favorable (cheaper) when buying;
/// seller-favorable (more expensive) when selling. Clamps at ±20%.
pub fn vendor_price_for(persuasion_ranks: u8, base_price: u32, side: TradeSide) -> u32 {
    let pct = (persuasion_ranks as i32)
        .saturating_mul(PERSUASION_PCT_PER_RANK)
        .clamp(0, PERSUASION_MAX_PCT);
    if pct == 0 {
        return base_price;
    }
    // Round to nearest copper; bias downward for a fractional half so the
    // buyer never overpays the displayed amount.
    let delta = ((base_price as i64) * pct as i64) / 100;
    match side {
        TradeSide::PlayerBuys => (base_price as i64 - delta).max(0) as u32,
        TradeSide::PlayerSells => (base_price as i64 + delta) as u32,
    }
}

/// Signed percent for `vendor_price_for`, matching `WareView.persuasion_modifier_pct`.
pub fn persuasion_modifier_pct(persuasion_ranks: u8, side: TradeSide) -> i8 {
    let pct = (persuasion_ranks as i32)
        .saturating_mul(PERSUASION_PCT_PER_RANK)
        .clamp(0, PERSUASION_MAX_PCT);
    match side {
        TradeSide::PlayerBuys => -(pct as i8),
        TradeSide::PlayerSells => pct as i8,
    }
}

/// Authoritative per-trade state. Lives only on the server, in `ActiveTrades`.
#[derive(Clone, Debug)]
pub struct TradeSession {
    pub session_id: TradeSessionId,
    pub participants: TradeParticipants,
    pub offers_a: Vec<TradeOfferEntry>,
    pub offers_b: Vec<TradeOfferEntry>,
    pub ready_a: bool,
    pub ready_b: bool,
    pub confirmed_a: bool,
    pub confirmed_b: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum TradeParticipants {
    PlayerToPlayer {
        a: PlayerId,
        b: PlayerId,
    },
    /// Player buys/sells against a shopkeeper NPC. The shopkeeper sits on
    /// `Side::B`; their offers come from the linked `Stockpile`.
    PlayerToShop {
        player: PlayerId,
        shop_object_id: u64,
    },
}

#[derive(Resource, Default)]
pub struct ActiveTrades {
    pub sessions: HashMap<TradeSessionId, TradeSession>,
    next_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

#[cfg_attr(not(feature = "server-sim"), allow(dead_code))]
impl ActiveTrades {
    pub fn allocate_session_id(&mut self) -> TradeSessionId {
        self.next_id += 1;
        self.next_id
    }

    /// Find the active session containing `player_id` (each player can be in
    /// at most one trade) plus which side they sit on.
    pub fn find_for_player(&self, player_id: PlayerId) -> Option<(TradeSessionId, Side)> {
        self.sessions
            .iter()
            .find_map(|(id, session)| match session.participants {
                TradeParticipants::PlayerToPlayer { a, b } => {
                    if a == player_id {
                        Some((*id, Side::A))
                    } else if b == player_id {
                        Some((*id, Side::B))
                    } else {
                        None
                    }
                }
                TradeParticipants::PlayerToShop { player, .. } => {
                    if player == player_id {
                        Some((*id, Side::A))
                    } else {
                        None
                    }
                }
            })
    }

    pub fn remove(&mut self, session_id: TradeSessionId) -> Option<TradeSession> {
        self.sessions.remove(&session_id)
    }
}

#[cfg_attr(not(feature = "server-sim"), allow(dead_code))]
impl TradeSession {
    /// Project this session for `viewing_player`'s perspective. Returns `None`
    /// if the player is not in this session.
    pub fn project_for(
        &self,
        viewing_player: PlayerId,
        partner_name: String,
        partner_kind: TradePartnerKind,
        wares: Option<Vec<WareView>>,
        sale_credit_copper: u32,
        total_owed_copper: u32,
    ) -> Option<ClientTradeView> {
        let (us, them, our_ready, their_ready, our_confirmed, their_confirmed) =
            match self.participants {
                TradeParticipants::PlayerToPlayer { a, b } => {
                    if viewing_player == a {
                        (
                            &self.offers_a,
                            &self.offers_b,
                            self.ready_a,
                            self.ready_b,
                            self.confirmed_a,
                            self.confirmed_b,
                        )
                    } else if viewing_player == b {
                        (
                            &self.offers_b,
                            &self.offers_a,
                            self.ready_b,
                            self.ready_a,
                            self.confirmed_b,
                            self.confirmed_a,
                        )
                    } else {
                        return None;
                    }
                }
                TradeParticipants::PlayerToShop { player, .. } => {
                    if viewing_player != player {
                        return None;
                    }
                    // Player always sits on Side::A in a shop session; the
                    // shop is Side::B.
                    (
                        &self.offers_a,
                        &self.offers_b,
                        self.ready_a,
                        self.ready_b,
                        self.confirmed_a,
                        self.confirmed_b,
                    )
                }
            };
        Some(ClientTradeView {
            session_id: self.session_id,
            partner_name,
            partner_kind,
            our_offers: us.clone(),
            their_offers: them.clone(),
            our_ready,
            their_ready,
            our_confirmed,
            their_confirmed,
            wares,
            sale_credit_copper,
            total_owed_copper,
        })
    }

    fn offers(&self, side: Side) -> &Vec<TradeOfferEntry> {
        match side {
            Side::A => &self.offers_a,
            Side::B => &self.offers_b,
        }
    }

    fn offers_mut(&mut self, side: Side) -> &mut Vec<TradeOfferEntry> {
        match side {
            Side::A => &mut self.offers_a,
            Side::B => &mut self.offers_b,
        }
    }

    fn set_ready(&mut self, side: Side, value: bool) {
        match side {
            Side::A => self.ready_a = value,
            Side::B => self.ready_b = value,
        }
    }

    fn ready(&self, side: Side) -> bool {
        match side {
            Side::A => self.ready_a,
            Side::B => self.ready_b,
        }
    }

    fn set_confirmed(&mut self, side: Side, value: bool) {
        match side {
            Side::A => self.confirmed_a = value,
            Side::B => self.confirmed_b = value,
        }
    }

    fn both_ready(&self) -> bool {
        self.ready_a && self.ready_b
    }

    fn both_confirmed(&self) -> bool {
        self.confirmed_a && self.confirmed_b
    }

    fn other_side(side: Side) -> Side {
        match side {
            Side::A => Side::B,
            Side::B => Side::A,
        }
    }

    /// Reset Ready+Confirm on the human-controlled sides whenever the offer
    /// list changes. In `PlayerToShop` sessions, side B is the shop NPC and
    /// is treated as always-ready / always-confirmed — the player drives both
    /// flags from a single side.
    fn reset_locks(&mut self) {
        self.ready_a = false;
        self.confirmed_a = false;
        if matches!(self.participants, TradeParticipants::PlayerToPlayer { .. }) {
            self.ready_b = false;
            self.confirmed_b = false;
        }
    }

    pub fn participant_player_ids(&self) -> (PlayerId, Option<PlayerId>) {
        match self.participants {
            TradeParticipants::PlayerToPlayer { a, b } => (a, Some(b)),
            TradeParticipants::PlayerToShop { player, .. } => (player, None),
        }
    }
}

/// Per-tick validation: any active trade whose participants have walked
/// apart (or whose partner has despawned) is aborted with a `Closed` UI
/// event so both sides' panels disappear cleanly. Runs in `CommandIntercept`
/// after `process_trade_commands` so the abort check sees the latest state.
#[cfg(feature = "server-sim")]
pub fn cleanup_invalid_trades(
    mut active_trades: ResMut<ActiveTrades>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    player_position_query: Query<(&PlayerIdentity, &SpaceResident, &TilePosition), With<Player>>,
    shopkeeper_query: Query<
        (&OverworldObject, &SpaceResident, &TilePosition),
        (With<Shopkeeper>, Without<Player>),
    >,
    floors: crate::world::column::FloorGeometryParam,
) {
    let mut to_close: Vec<(TradeSessionId, TradeOutcome, Vec<PlayerId>)> = Vec::new();

    for (session_id, session) in active_trades.sessions.iter() {
        let (outcome, recipients) = match session.participants {
            TradeParticipants::PlayerToPlayer { a, b } => {
                let pos_a = player_position_query
                    .iter()
                    .find(|(identity, _, _)| identity.id == a)
                    .map(|(_, resident, tile)| (resident.space_id, *tile));
                let pos_b = player_position_query
                    .iter()
                    .find(|(identity, _, _)| identity.id == b)
                    .map(|(_, resident, tile)| (resident.space_id, *tile));
                match (pos_a, pos_b) {
                    (Some((space_a, tile_a)), Some((space_b, tile_b))) => {
                        if space_a != space_b || !floors.reachable(&tile_a, &tile_b, space_a) {
                            (TradeOutcome::OutOfRange, vec![a, b])
                        } else {
                            continue;
                        }
                    }
                    _ => (TradeOutcome::PartnerDisconnected, vec![a, b]),
                }
            }
            TradeParticipants::PlayerToShop {
                player,
                shop_object_id,
            } => {
                let pos_p = player_position_query
                    .iter()
                    .find(|(identity, _, _)| identity.id == player)
                    .map(|(_, resident, tile)| (resident.space_id, *tile));
                let pos_shop = shopkeeper_query
                    .iter()
                    .find(|(object, _, _)| object.object_id == shop_object_id)
                    .map(|(_, resident, tile)| (resident.space_id, *tile));
                match (pos_p, pos_shop) {
                    (Some((space_p, tile_p)), Some((space_s, tile_s))) => {
                        if space_p != space_s || !floors.reachable(&tile_p, &tile_s, space_p) {
                            (TradeOutcome::OutOfRange, vec![player])
                        } else {
                            continue;
                        }
                    }
                    (Some(_), None) => (TradeOutcome::PartnerDisconnected, vec![player]),
                    (None, _) => continue,
                }
            }
        };
        to_close.push((*session_id, outcome, recipients));
    }

    for (session_id, outcome, recipients) in to_close {
        active_trades.remove(session_id);
        for player in recipients {
            ui_events.push(
                player,
                GameUiEvent::CloseTradePanel {
                    session_id,
                    outcome,
                },
            );
        }
    }
}

/// Drains all `Trade*` `GameCommand` variants from `PendingGameCommands` and
/// applies them to `ActiveTrades` + the involved players' inventories. Mirrors
/// the `process_dialog_commands` / `process_rotate_commands` pattern: scheduled
/// in `CommandIntercept` so the variants never reach `process_game_commands`.
#[cfg(feature = "server-sim")]
pub fn process_trade_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut active_trades: ResMut<ActiveTrades>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    definitions: Res<OverworldObjectDefinitions>,
    mut player_queries: ParamSet<(
        Query<
            (
                &PlayerIdentity,
                &SpaceResident,
                &TilePosition,
                &OverworldObject,
            ),
            With<Player>,
        >,
        PlayerActorQuery,
    )>,
    max_carry_query: Query<&MaxCarryWeight, With<Player>>,
    shopkeeper_query: Query<
        (
            &OverworldObject,
            &SpaceResident,
            &TilePosition,
            Option<&crate::npc::guilt::CrimeMemory>,
        ),
        (With<Shopkeeper>, Without<Player>),
    >,
    mut stockpile_query: Query<(&OverworldObject, &mut Stockpile)>,
    skill_query: Query<(&PlayerIdentity, &crate::player::skills::SkillSheet), With<Player>>,
    floors: crate::world::column::FloorGeometryParam,
) {
    for (queued_player_id, command) in pending_commands.drain_matching(|command| match command {
        claimed @ (GameCommand::InitiateTrade { .. }
        | GameCommand::OfferTradeItem { .. }
        | GameCommand::WithdrawTradeItem { .. }
        | GameCommand::ToggleTradeReady { .. }
        | GameCommand::ConfirmTrade { .. }
        | GameCommand::CancelTrade { .. }
        | GameCommand::BrowseShopBuy { .. }) => Ok(claimed),
        other => Err(other),
    }) {
        let acting_player_id = match queued_player_id {
            Some(id) => id,
            None => {
                // Embedded mode: trade commands target the single local player.
                player_queries
                    .p0()
                    .iter()
                    .next()
                    .map(|(identity, _, _, _)| identity.id)
                    .unwrap_or(PlayerId(0))
            }
        };

        match command {
            GameCommand::InitiateTrade { target } => {
                handle_initiate_trade(
                    acting_player_id,
                    target,
                    &mut active_trades,
                    &mut ui_events,
                    &player_queries.p0(),
                    &shopkeeper_query,
                    floors.geometry(),
                );
            }
            GameCommand::OfferTradeItem {
                session_id,
                source,
                quantity,
            } => {
                handle_offer_trade_item(
                    acting_player_id,
                    session_id,
                    source,
                    quantity,
                    &mut active_trades,
                    &mut player_queries.p1(),
                );
            }
            GameCommand::WithdrawTradeItem {
                session_id,
                offer_index,
            } => {
                handle_withdraw_trade_item(
                    acting_player_id,
                    session_id,
                    offer_index,
                    &mut active_trades,
                );
            }
            GameCommand::ToggleTradeReady { session_id } => {
                handle_toggle_trade_ready(acting_player_id, session_id, &mut active_trades);
            }
            GameCommand::ConfirmTrade { session_id } => {
                handle_confirm_trade(
                    acting_player_id,
                    session_id,
                    &mut active_trades,
                    &mut ui_events,
                    &definitions,
                    &mut player_queries.p1(),
                    &max_carry_query,
                    &mut stockpile_query,
                    &skill_query,
                );
            }
            GameCommand::CancelTrade { session_id } => {
                handle_cancel_trade(
                    acting_player_id,
                    session_id,
                    &mut active_trades,
                    &mut ui_events,
                );
            }
            GameCommand::BrowseShopBuy {
                session_id,
                ware_index,
                quantity,
            } => {
                handle_browse_shop_buy(
                    acting_player_id,
                    session_id,
                    ware_index,
                    quantity,
                    &mut active_trades,
                    &definitions,
                    &mut player_queries.p1(),
                    &stockpile_query,
                );
            }
            // The matcher above only claims the trade/shop variants.
            _ => {}
        }
    }
}

#[cfg(feature = "server-sim")]
fn handle_initiate_trade(
    acting_player_id: PlayerId,
    target: TradeTarget,
    active_trades: &mut ActiveTrades,
    ui_events: &mut PendingGameUiEvents,
    player_position_query: &Query<
        (
            &PlayerIdentity,
            &SpaceResident,
            &TilePosition,
            &OverworldObject,
        ),
        With<Player>,
    >,
    shopkeeper_query: &Query<
        (
            &OverworldObject,
            &SpaceResident,
            &TilePosition,
            Option<&crate::npc::guilt::CrimeMemory>,
        ),
        (With<Shopkeeper>, Without<Player>),
    >,
    geometry: crate::world::column::FloorGeometry<'_>,
) {
    if active_trades.find_for_player(acting_player_id).is_some() {
        bevy::log::debug!(
            "InitiateTrade rejected: player {:?} already in a trade",
            acting_player_id
        );
        return;
    }

    // Resolve the acting player's position.
    let acting_pos = player_position_query
        .iter()
        .find(|(identity, _, _, _)| identity.id == acting_player_id)
        .map(|(_, resident, tile, _)| (resident.space_id, *tile));
    let Some((acting_space, acting_tile)) = acting_pos else {
        return;
    };

    match target {
        TradeTarget::Player {
            object_id: target_object_id,
        } => {
            let target = player_position_query
                .iter()
                .find(|(_, resident, _, object)| {
                    resident.space_id == acting_space && object.object_id == target_object_id
                });
            let Some((target_identity, _, target_tile, _)) = target else {
                bevy::log::debug!(
                    "InitiateTrade: target object {target_object_id} is not a player in this space"
                );
                return;
            };
            let target_player_id = target_identity.id;

            if target_player_id == acting_player_id {
                return;
            }

            if active_trades.find_for_player(target_player_id).is_some() {
                bevy::log::debug!(
                    "InitiateTrade rejected: target player {:?} already in a trade",
                    target_player_id
                );
                return;
            }

            if !geometry.reachable(&acting_tile, target_tile, acting_space) {
                bevy::log::debug!("InitiateTrade rejected: target out of reach");
                return;
            }

            let session_id = active_trades.allocate_session_id();
            let session = TradeSession {
                session_id,
                participants: TradeParticipants::PlayerToPlayer {
                    a: acting_player_id,
                    b: target_player_id,
                },
                offers_a: Vec::new(),
                offers_b: Vec::new(),
                ready_a: false,
                ready_b: false,
                confirmed_a: false,
                confirmed_b: false,
            };
            active_trades.sessions.insert(session_id, session);

            ui_events.push(acting_player_id, GameUiEvent::OpenTradePanel { session_id });
            ui_events.push(target_player_id, GameUiEvent::OpenTradePanel { session_id });
        }
        TradeTarget::Shopkeeper {
            object_id: shop_object_id,
        } => {
            let shopkeeper = shopkeeper_query.iter().find(|(object, resident, _, _)| {
                resident.space_id == acting_space && object.object_id == shop_object_id
            });
            let Some((_, _, shop_tile, shop_guilt)) = shopkeeper else {
                bevy::log::debug!(
                    "InitiateTrade: target object {shop_object_id} is not a shopkeeper"
                );
                return;
            };
            if !geometry.reachable(&acting_tile, shop_tile, acting_space) {
                bevy::log::debug!("InitiateTrade rejected: shopkeeper out of reach");
                return;
            }
            // Guilt gate, mirroring the Talk path: a merchant who holds a
            // grudge won't open the ledger. Said out loud, since the rejection
            // is otherwise silent on the wire.
            if crate::npc::guilt::refuses_interaction(shop_guilt, acting_player_id) {
                bevy::log::debug!("InitiateTrade rejected: shopkeeper refuses a guilty player");
                ui_events.push_broadcast_near(
                    acting_space,
                    *shop_tile,
                    GameUiEvent::SpeechBubble {
                        speaker_object_id: shop_object_id,
                        text: crate::npc::guilt::REFUSAL_LINE.to_owned(),
                        style: crate::game::resources::SpeechBubbleStyle::Say,
                    },
                );
                return;
            }

            let session_id = active_trades.allocate_session_id();
            let session = TradeSession {
                session_id,
                participants: TradeParticipants::PlayerToShop {
                    player: acting_player_id,
                    shop_object_id,
                },
                offers_a: Vec::new(),
                offers_b: Vec::new(),
                ready_a: false,
                // Shop is always-ready and always-confirmed: only the player
                // drives those flags. Keeping shop's flags `true` from the
                // start lets the standard `both_ready / both_confirmed`
                // checks work uniformly.
                ready_b: true,
                confirmed_a: false,
                confirmed_b: true,
            };
            active_trades.sessions.insert(session_id, session);

            ui_events.push(acting_player_id, GameUiEvent::OpenTradePanel { session_id });
        }
    }
}

#[cfg(feature = "server-sim")]
fn handle_offer_trade_item(
    acting_player_id: PlayerId,
    session_id: TradeSessionId,
    source: ItemSlotRef,
    quantity: u32,
    active_trades: &mut ActiveTrades,
    player_inventory_query: &mut PlayerActorQuery,
) {
    let Some(side) = side_for_session_player(active_trades, session_id, acting_player_id) else {
        return;
    };

    if quantity == 0 {
        return;
    }

    // Look up the inventory stack at `source`.
    let player_entity = player_inventory_query
        .iter()
        .find(|(_, identity, _, _, _, _, _, _, _)| identity.id == acting_player_id)
        .map(|(entity, _, _, _, _, _, _, _, _)| entity);
    let Some(player_entity) = player_entity else {
        return;
    };

    let Ok((_, _, inventory, _, _, _, _, _, _)) = player_inventory_query.get(player_entity) else {
        return;
    };
    let Some((type_id, properties, available)) = read_player_slot(&source, inventory) else {
        return;
    };

    // Calculate the quantity already promised in existing offers from the same
    // source slot — prevents double-offering the same items.
    let already_offered: u32 = {
        let session = active_trades
            .sessions
            .get(&session_id)
            .expect("session resolved earlier");
        session
            .offers(side)
            .iter()
            .filter_map(|entry| match &entry.source {
                OfferSource::PlayerSlot(slot) if slot == &source => Some(entry.quantity),
                _ => None,
            })
            .sum()
    };

    let actual_quantity = quantity.min(available.saturating_sub(already_offered));
    if actual_quantity == 0 {
        return;
    }

    let session = active_trades
        .sessions
        .get_mut(&session_id)
        .expect("session resolved earlier");
    session.reset_locks();
    // If the same source already has an entry, merge into it. Otherwise push.
    if let Some(existing) = session
        .offers_mut(side)
        .iter_mut()
        .find(|entry| matches!(&entry.source, OfferSource::PlayerSlot(slot) if slot == &source))
    {
        existing.quantity = existing.quantity.saturating_add(actual_quantity);
    } else {
        session.offers_mut(side).push(TradeOfferEntry {
            source: OfferSource::PlayerSlot(source),
            type_id,
            properties,
            quantity: actual_quantity,
        });
    }
}

#[cfg(feature = "server-sim")]
fn handle_withdraw_trade_item(
    acting_player_id: PlayerId,
    session_id: TradeSessionId,
    offer_index: usize,
    active_trades: &mut ActiveTrades,
) {
    let Some(side) = side_for_session_player(active_trades, session_id, acting_player_id) else {
        return;
    };
    let session = active_trades
        .sessions
        .get_mut(&session_id)
        .expect("session resolved earlier");
    if offer_index >= session.offers(side).len() {
        return;
    }
    session.offers_mut(side).remove(offer_index);
    session.reset_locks();
}

#[cfg(feature = "server-sim")]
fn handle_toggle_trade_ready(
    acting_player_id: PlayerId,
    session_id: TradeSessionId,
    active_trades: &mut ActiveTrades,
) {
    let Some(side) = side_for_session_player(active_trades, session_id, acting_player_id) else {
        return;
    };
    let session = active_trades
        .sessions
        .get_mut(&session_id)
        .expect("session resolved earlier");
    let new_state = !session.ready(side);
    session.set_ready(side, new_state);
    if !new_state {
        // Un-readying also clears confirms (you cannot be confirmed without
        // being ready).
        session.set_confirmed(side, false);
        session.set_confirmed(TradeSession::other_side(side), false);
    }
}

#[cfg(feature = "server-sim")]
fn handle_confirm_trade(
    acting_player_id: PlayerId,
    session_id: TradeSessionId,
    active_trades: &mut ActiveTrades,
    ui_events: &mut PendingGameUiEvents,
    definitions: &OverworldObjectDefinitions,
    player_inventory_query: &mut PlayerActorQuery,
    max_carry_query: &Query<&MaxCarryWeight, With<Player>>,
    stockpile_query: &mut Query<(&OverworldObject, &mut Stockpile)>,
    skill_query: &Query<(&PlayerIdentity, &crate::player::skills::SkillSheet), With<Player>>,
) {
    let Some(side) = side_for_session_player(active_trades, session_id, acting_player_id) else {
        return;
    };

    // Scope the mutable borrow on `active_trades.sessions` so we can call
    // `active_trades.remove(...)` after the commit, with the borrow released.
    let session_snapshot = {
        let session = active_trades
            .sessions
            .get_mut(&session_id)
            .expect("session resolved earlier");
        if !session.both_ready() {
            // Confirming before both Ready does nothing (UI shouldn't allow).
            return;
        }
        session.set_confirmed(side, true);
        if !session.both_confirmed() {
            return;
        }
        session.clone()
    };

    let (result, players_to_notify): (CommitResult, Vec<PlayerId>) =
        match session_snapshot.participants {
            TradeParticipants::PlayerToPlayer { a, b } => {
                let ok = commit_player_to_player_trade(
                    &session_snapshot,
                    a,
                    b,
                    definitions,
                    player_inventory_query,
                    max_carry_query,
                );
                let result = if ok {
                    CommitResult::Completed
                } else {
                    CommitResult::Failed
                };
                (result, vec![a, b])
            }
            TradeParticipants::PlayerToShop {
                player,
                shop_object_id,
            } => {
                let persuasion_ranks = skill_query
                    .iter()
                    .find(|(identity, _)| identity.id == player)
                    .map(|(_, sheet)| sheet.rank(crate::player::skills::Skill::Persuasion))
                    .unwrap_or(0);
                let ok = commit_player_to_shop_trade(
                    &session_snapshot,
                    player,
                    shop_object_id,
                    definitions,
                    player_inventory_query,
                    max_carry_query,
                    stockpile_query,
                    persuasion_ranks,
                );
                (ok, vec![player])
            }
        };

    if result == CommitResult::Refused {
        // Keep the session alive and just unlock it, so the player can adjust
        // the basket instead of losing it. The mutation makes the projection
        // re-emit the trade state, which un-sticks the panel's Ready/Confirm.
        if let Some(session) = active_trades.sessions.get_mut(&session_id) {
            session.reset_locks();
        }
        return;
    }

    active_trades.remove(session_id);

    let outcome = if result == CommitResult::Completed {
        TradeOutcome::Completed
    } else {
        TradeOutcome::Cancelled
    };

    for player in players_to_notify {
        ui_events.push(
            player,
            GameUiEvent::CloseTradePanel {
                session_id,
                outcome,
            },
        );
    }
}

/// Append a ware to the shop side (Side::B) of an active trade. The player
/// is responsible for adding their own coin offers to Side::A — the merchant
/// only validates the totals at commit time (`commit_player_to_shop_trade`).
#[cfg(feature = "server-sim")]
fn handle_browse_shop_buy(
    acting_player_id: PlayerId,
    session_id: TradeSessionId,
    ware_index: usize,
    quantity: u32,
    active_trades: &mut ActiveTrades,
    _definitions: &OverworldObjectDefinitions,
    player_inventory_query: &mut PlayerActorQuery,
    stockpile_query: &Query<(&OverworldObject, &mut Stockpile)>,
) {
    if quantity == 0 {
        return;
    }
    let Some(_side) = side_for_session_player(active_trades, session_id, acting_player_id) else {
        return;
    };

    let session = active_trades
        .sessions
        .get(&session_id)
        .expect("session resolved earlier");
    let TradeParticipants::PlayerToShop { shop_object_id, .. } = session.participants else {
        return;
    };

    let (ware_type_id, stock_remaining) = match stockpile_query
        .iter()
        .find(|(object, _)| object.object_id == shop_object_id)
    {
        Some((_, stockpile)) => match stockpile.wares.get(ware_index) {
            Some(entry) => (
                entry.type_id.clone(),
                match entry.stock {
                    StockMode::Infinite => None,
                    StockMode::Finite(n) => Some(n),
                },
            ),
            None => return,
        },
        None => return,
    };
    let already_offered: u32 = session
        .offers_b
        .iter()
        .filter_map(|entry| match entry.source {
            OfferSource::Stockpile { ware_index: idx } if idx == ware_index => Some(entry.quantity),
            _ => None,
        })
        .sum();

    let player_entity = player_inventory_query
        .iter()
        .find(|(_, identity, _, _, _, _, _, _, _)| identity.id == acting_player_id)
        .map(|(entity, _, _, _, _, _, _, _, _)| entity);
    let Some(player_entity) = player_entity else {
        return;
    };

    if let Some(remaining) = stock_remaining {
        if remaining < already_offered.saturating_add(quantity) {
            if let Ok((_, _, _, mut chat_log, _, _, _, _, _)) =
                player_inventory_query.get_mut(player_entity)
            {
                chat_log.push_narrator("Out of stock.");
            }
            return;
        }
    }

    let session = active_trades
        .sessions
        .get_mut(&session_id)
        .expect("session resolved earlier");
    session.reset_locks();

    if let Some(existing) = session
        .offers_b
        .iter_mut()
        .find(|entry| matches!(entry.source, OfferSource::Stockpile { ware_index: idx } if idx == ware_index))
    {
        existing.quantity = existing.quantity.saturating_add(quantity);
    } else {
        session.offers_b.push(TradeOfferEntry {
            source: OfferSource::Stockpile { ware_index },
            type_id: ware_type_id,
            properties: ObjectProperties::new(),
            quantity,
        });
    }
}

/// Render a copper-denominated price as `"3g 5s 4c"` (parts that are zero
/// are omitted; the all-zero case prints `0c`).
#[cfg(feature = "server-sim")]
fn format_copper(copper: u32) -> String {
    let (g, s, c) = crate::game::currency::split(copper);
    let mut out = String::new();
    if g > 0 {
        out.push_str(&format!("{}g ", g));
    }
    if s > 0 {
        out.push_str(&format!("{}s ", s));
    }
    if c > 0 || (g == 0 && s == 0) {
        out.push_str(&format!("{}c", c));
    }
    out.trim_end().to_owned()
}

/// Outcome of a commit attempt.
///
/// `Refused` is the player-fixable case (can't afford it, nowhere to put the
/// change): nothing is committed, but the session survives so the basket the
/// player assembled isn't thrown away — they can drop an item and re-confirm.
/// `Failed` is a hard abort (stale offers, missing shop) and cancels.
#[cfg(feature = "server-sim")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommitResult {
    Completed,
    Failed,
    Refused,
}

#[cfg(feature = "server-sim")]
fn commit_player_to_shop_trade(
    session: &TradeSession,
    player: PlayerId,
    shop_object_id: u64,
    definitions: &OverworldObjectDefinitions,
    player_inventory_query: &mut PlayerActorQuery,
    max_carry_query: &Query<&MaxCarryWeight, With<Player>>,
    stockpile_query: &mut Query<(&OverworldObject, &mut Stockpile)>,
    persuasion_ranks: u8,
) -> CommitResult {
    let entity = player_inventory_query
        .iter()
        .find(|(_, identity, _, _, _, _, _, _, _)| identity.id == player)
        .map(|(e, _, _, _, _, _, _, _, _)| e);
    let Some(entity) = entity else {
        return CommitResult::Failed;
    };
    let mut inv = match player_inventory_query.get(entity) {
        Ok((_, _, inventory, _, _, _, _, _, _)) => inventory.clone(),
        Err(_) => return CommitResult::Failed,
    };
    let max_carry = max_carry_query.get(entity).copied().unwrap_or_default();

    // Validate player coin offers against current inventory.
    if !validate_offers_against(&session.offers_a, &inv) {
        return CommitResult::Failed;
    }

    // Total price the merchant is asking for (sum of ware price * quantity).
    let mut total_owed_copper: u32 = 0;
    for offer in &session.offers_b {
        let OfferSource::Stockpile { ware_index } = &offer.source else {
            return CommitResult::Failed;
        };
        let stocks = stockpile_query
            .iter()
            .find(|(object, _)| object.object_id == shop_object_id);
        let Some((_, stockpile)) = stocks else {
            return CommitResult::Failed;
        };
        let Some(entry) = stockpile.wares.get(*ware_index) else {
            return CommitResult::Failed;
        };
        if entry.type_id != offer.type_id {
            return CommitResult::Failed;
        }
        if let StockMode::Finite(n) = entry.stock {
            if n < offer.quantity {
                return CommitResult::Failed;
            }
        }
        let modified_price =
            vendor_price_for(persuasion_ranks, entry.price_copper, TradeSide::PlayerBuys);
        total_owed_copper =
            total_owed_copper.saturating_add(modified_price.saturating_mul(offer.quantity));
    }

    // Sum what the player is handing over. Coins count at face value; anything
    // else is *sold* to the merchant at its market value (see
    // `sell_offer_value_copper`). Items with no `value_copper` still come
    // across for free, which is the long-standing behaviour for putting
    // junk in a merchant's column.
    let total_offered_copper: u32 = session
        .offers_a
        .iter()
        .map(|entry| offer_credit_copper(entry, definitions, persuasion_ranks))
        .fold(0u32, |acc, v| acc.saturating_add(v));

    // Remove player's offers from the inventory snapshot. This runs *before*
    // the purse is tapped and the change is minted, so the slots freed by the
    // sold goods are available to hold the coin that replaces them — and so
    // coins the player dragged in are not counted twice (once as an offer,
    // once as purse).
    if !remove_offered_from(&session.offers_a, &mut inv) {
        return CommitResult::Failed;
    }

    // Whatever the offered goods and coin don't cover, the merchant takes
    // straight out of the purse — no coin-dragging required for a plain
    // purchase. `spend_copper` melts the purse and re-mints the change on its
    // own clone, so a failure here leaves `inv` untouched.
    let purse_paid_copper = total_owed_copper.saturating_sub(total_offered_copper);
    if purse_paid_copper > 0
        && !crate::game::currency::spend_copper(&mut inv, purse_paid_copper, definitions)
    {
        if let Ok((_, _, _, mut chat_log, _, _, _, _, _)) = player_inventory_query.get_mut(entity) {
            chat_log.push_narrator(format!(
                "The merchant frowns. \"Short by {} — you carry only {}.\"",
                format_copper(purse_paid_copper),
                format_copper(crate::game::currency::purse_total_copper(&inv))
            ));
        }
        return CommitResult::Refused;
    }

    // Insert the wares into the snapshot.
    if !insert_offers_into(&session.offers_b, &mut inv, definitions, &max_carry) {
        return CommitResult::Failed;
    }

    // Pay out the difference. `deposit_copper` may leave partial coin behind
    // when it fails, but `inv` is a snapshot that is only committed below, so
    // bailing here leaves the player's real inventory untouched — better than
    // taking their goods and swallowing the payment.
    let change_copper = total_offered_copper.saturating_sub(total_owed_copper);
    if change_copper > 0
        && !crate::game::currency::deposit_copper(&mut inv, change_copper, definitions)
    {
        if let Ok((_, _, _, mut chat_log, _, _, _, _, _)) = player_inventory_query.get_mut(entity) {
            chat_log.push_narrator(format!(
                "The merchant counts out {} and waits — you've nowhere to put it.",
                format_copper(change_copper)
            ));
        }
        return CommitResult::Refused;
    }

    // Commit: write the inventory snapshot back and decrement finite stocks.
    if let Ok((_, _, mut inventory, mut chat_log, _, _, _, _, _)) =
        player_inventory_query.get_mut(entity)
    {
        *inventory = inv;
        if change_copper > 0 {
            chat_log.push_narrator(format!(
                "Trade complete. The merchant counts out {}.",
                format_copper(change_copper)
            ));
        } else if purse_paid_copper > 0 {
            chat_log.push_narrator(format!(
                "Trade complete. The merchant takes {} from your purse.",
                format_copper(purse_paid_copper)
            ));
        } else {
            chat_log.push_narrator("Trade complete.");
        }
    }

    if let Some((_, mut stockpile)) = stockpile_query
        .iter_mut()
        .find(|(object, _)| object.object_id == shop_object_id)
    {
        for offer in &session.offers_b {
            if let OfferSource::Stockpile { ware_index } = &offer.source {
                if let Some(entry) = stockpile.wares.get_mut(*ware_index) {
                    let _ = entry.try_take(offer.quantity);
                }
            }
        }
        // What the player sold goes onto the shelf, so the merchant visibly
        // resells it. Priced at full market value (he sells at price, buys at
        // half — that spread is his margin). Coins are not wares.
        for offer in &session.offers_a {
            let Some(def) = definitions.get(&offer.type_id) else {
                continue;
            };
            let Some(value) = def.value_copper.filter(|v| *v > 0) else {
                continue;
            };
            match stockpile
                .wares
                .iter_mut()
                .find(|w| w.type_id == offer.type_id)
            {
                Some(existing) => existing.restock(offer.quantity),
                None => stockpile.wares.push(StockEntry {
                    type_id: offer.type_id.clone(),
                    price_copper: value,
                    stock: StockMode::Finite(offer.quantity),
                }),
            }
        }
    }
    CommitResult::Completed
}

/// What the merchant credits the player for one offer entry, in copper.
///
/// Coins are face value. Everything else is a sale at half the item's
/// `value_copper`, nudged in the player's favour by Persuasion — the first
/// live use of [`TradeSide::PlayerSells`].
///
/// Ungated on purpose: the projection calls it to fill
/// `ClientTradeView::sale_credit_copper`, so the previewed total and the
/// committed payout come from one function and cannot drift.
pub fn offer_credit_copper(
    entry: &TradeOfferEntry,
    definitions: &OverworldObjectDefinitions,
    persuasion_ranks: u8,
) -> u32 {
    match entry.type_id.as_str() {
        COPPER_TYPE_ID => entry.quantity,
        SILVER_TYPE_ID => entry.quantity.saturating_mul(COPPER_PER_SILVER),
        GOLD_TYPE_ID => entry.quantity.saturating_mul(COPPER_PER_GOLD),
        type_id => {
            // Priced per stack, not per item — see `sell_value_copper`.
            let base = definitions
                .get(type_id)
                .map(|def| def.sell_value_copper(entry.quantity))
                .unwrap_or(0);
            if base == 0 {
                return 0;
            }
            vendor_price_for(persuasion_ranks, base, TradeSide::PlayerSells)
        }
    }
}

#[cfg(feature = "server-sim")]
fn handle_cancel_trade(
    acting_player_id: PlayerId,
    session_id: TradeSessionId,
    active_trades: &mut ActiveTrades,
    ui_events: &mut PendingGameUiEvents,
) {
    let Some(_side) = side_for_session_player(active_trades, session_id, acting_player_id) else {
        return;
    };
    let Some(session) = active_trades.remove(session_id) else {
        return;
    };
    let (player_a, player_b_opt) = session.participant_player_ids();
    ui_events.push(
        player_a,
        GameUiEvent::CloseTradePanel {
            session_id,
            outcome: TradeOutcome::Cancelled,
        },
    );
    if let Some(player_b) = player_b_opt {
        ui_events.push(
            player_b,
            GameUiEvent::CloseTradePanel {
                session_id,
                outcome: TradeOutcome::Cancelled,
            },
        );
    }
}

#[cfg(feature = "server-sim")]
fn side_for_session_player(
    active_trades: &ActiveTrades,
    session_id: TradeSessionId,
    player_id: PlayerId,
) -> Option<Side> {
    let session = active_trades.sessions.get(&session_id)?;
    match session.participants {
        TradeParticipants::PlayerToPlayer { a, b } => {
            if a == player_id {
                Some(Side::A)
            } else if b == player_id {
                Some(Side::B)
            } else {
                None
            }
        }
        TradeParticipants::PlayerToShop { player, .. } => {
            if player == player_id {
                Some(Side::A)
            } else {
                None
            }
        }
    }
}

/// Read the current contents of a player's inventory slot. Only the three
/// player-personal slot kinds are accepted (Backpack / Equipment /
/// PouchInBackpack); world-container references are rejected so trades can
/// never reach into shared chests.
#[cfg(feature = "server-sim")]
fn read_player_slot(
    slot: &ItemSlotRef,
    inventory: &InventoryState,
) -> Option<(String, ObjectProperties, u32)> {
    match slot {
        ItemSlotRef::Equipment(equipment_slot) => {
            let item = inventory.equipment_item(*equipment_slot)?;
            // Ammo slots track quantity separately on the inventory; other
            // equipment slots are 1-of-a-kind.
            let qty = if matches!(
                equipment_slot,
                crate::world::object_definitions::EquipmentSlot::Ammo
            ) {
                inventory.ammo_quantity.max(1)
            } else {
                1
            };
            Some((item.type_id.clone(), item.properties.clone(), qty))
        }
        _ => {
            let stack = crate::game::slots::player_stack_slot(inventory, *slot)?.as_ref()?;
            Some((
                stack.type_id.clone(),
                stack.properties.clone(),
                stack.quantity,
            ))
        }
    }
}

/// Atomically transfer all offered items between two players. Returns `true`
/// on success; on validation failure (source no longer resolves, weight cap,
/// or no inventory space) returns `false` and leaves both inventories
/// unchanged.
#[cfg(feature = "server-sim")]
fn commit_player_to_player_trade(
    session: &TradeSession,
    player_a: PlayerId,
    player_b: PlayerId,
    definitions: &OverworldObjectDefinitions,
    player_inventory_query: &mut PlayerActorQuery,
    max_carry_query: &Query<&MaxCarryWeight, With<Player>>,
) -> bool {
    // Resolve player entities.
    let entity_a = player_inventory_query
        .iter()
        .find(|(_, identity, _, _, _, _, _, _, _)| identity.id == player_a)
        .map(|(entity, _, _, _, _, _, _, _, _)| entity);
    let entity_b = player_inventory_query
        .iter()
        .find(|(_, identity, _, _, _, _, _, _, _)| identity.id == player_b)
        .map(|(entity, _, _, _, _, _, _, _, _)| entity);
    let (Some(entity_a), Some(entity_b)) = (entity_a, entity_b) else {
        return false;
    };

    // Snapshot both inventories — we apply changes to the snapshot, validate,
    // then write back. This gives us atomicity.
    let mut inv_a = match player_inventory_query.get(entity_a) {
        Ok((_, _, inventory, _, _, _, _, _, _)) => inventory.clone(),
        Err(_) => return false,
    };
    let mut inv_b = match player_inventory_query.get(entity_b) {
        Ok((_, _, inventory, _, _, _, _, _, _)) => inventory.clone(),
        Err(_) => return false,
    };
    let max_carry_a = max_carry_query.get(entity_a).copied().unwrap_or_default();
    let max_carry_b = max_carry_query.get(entity_b).copied().unwrap_or_default();

    // Step 1: validate that every offer source still resolves to at least the
    // promised quantity.
    if !validate_offers_against(&session.offers_a, &inv_a) {
        return false;
    }
    if !validate_offers_against(&session.offers_b, &inv_b) {
        return false;
    }

    // Step 2: remove offered items from both inventories.
    if !remove_offered_from(&session.offers_a, &mut inv_a) {
        return false;
    }
    if !remove_offered_from(&session.offers_b, &mut inv_b) {
        return false;
    }

    // Step 3: insert opposite side's offers into each inventory, respecting
    // weight caps. If either insert fails we abort.
    if !insert_offers_into(&session.offers_b, &mut inv_a, definitions, &max_carry_a) {
        return false;
    }
    if !insert_offers_into(&session.offers_a, &mut inv_b, definitions, &max_carry_b) {
        return false;
    }

    // Commit: write the snapshots back.
    if let Ok((_, _, mut inventory, mut chat_log, _, _, _, _, _)) =
        player_inventory_query.get_mut(entity_a)
    {
        *inventory = inv_a;
        chat_log.push_narrator("Trade complete.");
    }
    if let Ok((_, _, mut inventory, mut chat_log, _, _, _, _, _)) =
        player_inventory_query.get_mut(entity_b)
    {
        *inventory = inv_b;
        chat_log.push_narrator("Trade complete.");
    }
    true
}

#[cfg(feature = "server-sim")]
fn validate_offers_against(offers: &[TradeOfferEntry], inventory: &InventoryState) -> bool {
    // Group offers by source slot and ensure the slot still holds enough
    // matching items. Stockpile-sourced offers are validated separately at
    // commit time and skipped here.
    let mut required: HashMap<ItemSlotRef, u32> = HashMap::new();
    for offer in offers {
        let OfferSource::PlayerSlot(slot) = &offer.source else {
            continue;
        };
        *required.entry(*slot).or_insert(0) += offer.quantity;
        let Some((type_id, _properties, available)) = read_player_slot(slot, inventory) else {
            return false;
        };
        if type_id != offer.type_id {
            return false;
        }
        if available < *required.get(slot).unwrap_or(&0) {
            return false;
        }
    }
    true
}

#[cfg(feature = "server-sim")]
fn remove_offered_from(offers: &[TradeOfferEntry], inventory: &mut InventoryState) -> bool {
    for offer in offers {
        let OfferSource::PlayerSlot(slot) = &offer.source else {
            // Stockpile-sourced offers don't come out of any inventory; the
            // shop-commit path decrements `Stockpile::stock` separately.
            continue;
        };
        if !decrement_player_slot(slot, offer.quantity, inventory) {
            return false;
        }
    }
    true
}

#[cfg(feature = "server-sim")]
fn decrement_player_slot(slot: &ItemSlotRef, amount: u32, inventory: &mut InventoryState) -> bool {
    match slot {
        ItemSlotRef::Equipment(equipment_slot) => {
            use crate::world::object_definitions::EquipmentSlot;
            if matches!(equipment_slot, EquipmentSlot::Ammo) {
                if inventory.ammo_quantity < amount {
                    return false;
                }
                inventory.ammo_quantity -= amount;
                if inventory.ammo_quantity == 0 {
                    inventory.take_equipment_item(*equipment_slot);
                }
                true
            } else {
                if amount != 1 {
                    return false;
                }
                inventory.take_equipment_item(*equipment_slot).is_some()
            }
        }
        ItemSlotRef::Container { .. } => false,
        _ => {
            let Some(option_slot) = crate::game::slots::player_stack_slot_mut(inventory, *slot)
            else {
                return false;
            };
            let Some(stack) = option_slot else {
                return false;
            };
            if stack.quantity < amount {
                return false;
            }
            stack.quantity -= amount;
            if stack.quantity == 0 {
                *option_slot = None;
            }
            true
        }
    }
}

/// Insert each offer's items into `inventory`, merging into existing stacks
/// where possible and respecting weight caps. Returns `false` if any item
/// can't be placed (no free slot or hard-cap exceeded).
#[cfg(feature = "server-sim")]
fn insert_offers_into(
    offers: &[TradeOfferEntry],
    inventory: &mut InventoryState,
    definitions: &OverworldObjectDefinitions,
    max_carry: &MaxCarryWeight,
) -> bool {
    for offer in offers {
        if !insert_one_offer(offer, inventory, definitions, max_carry) {
            return false;
        }
    }
    true
}

#[cfg(feature = "server-sim")]
fn insert_one_offer(
    offer: &TradeOfferEntry,
    inventory: &mut InventoryState,
    definitions: &OverworldObjectDefinitions,
    max_carry: &MaxCarryWeight,
) -> bool {
    let Some(definition) = definitions.get(&offer.type_id) else {
        return false;
    };
    let max_stack = definition.max_stack_size.max(1);
    let per_unit_weight = definition.weight;
    let mut remaining = offer.quantity;
    let mut current_weight = inventory.total_weight(definitions);

    if max_stack > 1 {
        for slot in inventory.backpack_slots.iter_mut() {
            if remaining == 0 {
                break;
            }
            let Some(stack) = slot else { continue };
            if stack.type_id != offer.type_id {
                continue;
            }
            let available = max_stack.saturating_sub(stack.quantity);
            if available == 0 {
                continue;
            }
            let take = remaining.min(available);
            if per_unit_weight > 0.0
                && current_weight + per_unit_weight * take as f32 > max_carry.hard_cap
            {
                return false;
            }
            stack.quantity += take;
            current_weight += per_unit_weight * take as f32;
            remaining -= take;
        }
    }

    while remaining > 0 {
        let Some(empty_index) = inventory
            .backpack_slots
            .iter()
            .position(|slot| slot.is_none())
        else {
            return false;
        };
        let take = if max_stack > 1 {
            remaining.min(max_stack)
        } else {
            1
        };
        if per_unit_weight > 0.0
            && current_weight + per_unit_weight * take as f32 > max_carry.hard_cap
        {
            return false;
        }
        let mut new_stack =
            InventoryStack::item(offer.type_id.clone(), offer.properties.clone(), take);
        if let Some(capacity) = definition.container_capacity {
            new_stack.contained_slots = Some(vec![None; capacity]);
        }
        // Shop stock arrives fully charged, mirroring `handle_give_item`.
        // A player-to-player offer of a partially-used item already carries
        // `charges_remaining` in its properties — leave that untouched.
        if let Some(max_charges) = definition.max_charges {
            if !definition.infinite_uses && new_stack.charges_remaining().is_none() {
                new_stack.set_charges_remaining(max_charges);
            }
        }
        inventory.backpack_slots[empty_index] = Some(new_stack);
        current_weight += per_unit_weight * take as f32;
        remaining -= take;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_take_handles_finite_and_infinite_stock() {
        let mut entry = StockEntry {
            type_id: "apple".to_owned(),
            price_copper: 4,
            stock: StockMode::Finite(3),
        };
        assert!(entry.try_take(2));
        assert!(matches!(entry.stock, StockMode::Finite(1)));
        assert!(!entry.try_take(2)); // would exceed remaining
        assert!(entry.try_take(1));
        assert!(matches!(entry.stock, StockMode::Finite(0)));

        let mut infinite = StockEntry {
            type_id: "apple".to_owned(),
            price_copper: 4,
            stock: StockMode::Infinite,
        };
        assert!(infinite.try_take(1_000_000));
        assert!(matches!(infinite.stock, StockMode::Infinite));
    }

    #[test]
    fn insert_one_offer_charges_fresh_stock_but_preserves_used_charges() {
        let definitions = OverworldObjectDefinitions::load_from_disk();
        let definition = definitions
            .get("wand_of_sparks")
            .expect("wand_of_sparks definition with max_charges");
        let max_charges = definition.max_charges.expect("wand declares max_charges");
        let max_carry = MaxCarryWeight {
            soft_cap: 1000.0,
            hard_cap: 1000.0,
        };

        // Shop stock (no properties) arrives fully charged, like /give.
        let mut inventory = InventoryState::default();
        let fresh_offer = TradeOfferEntry {
            source: OfferSource::Stockpile { ware_index: 0 },
            type_id: "wand_of_sparks".to_owned(),
            properties: ObjectProperties::new(),
            quantity: 1,
        };
        assert!(insert_one_offer(
            &fresh_offer,
            &mut inventory,
            &definitions,
            &max_carry
        ));
        let stack = inventory.backpack_slots[0].as_ref().unwrap();
        assert_eq!(stack.charges_remaining(), Some(max_charges));

        // A traded partially-used wand keeps its recorded charges.
        let mut inventory = InventoryState::default();
        let mut used_properties = ObjectProperties::new();
        used_properties.insert(
            crate::player::components::CHARGES_KEY.to_owned(),
            "7".to_owned(),
        );
        let used_offer = TradeOfferEntry {
            source: OfferSource::PlayerSlot(ItemSlotRef::Backpack(0)),
            type_id: "wand_of_sparks".to_owned(),
            properties: used_properties,
            quantity: 1,
        };
        assert!(insert_one_offer(
            &used_offer,
            &mut inventory,
            &definitions,
            &max_carry
        ));
        let stack = inventory.backpack_slots[0].as_ref().unwrap();
        assert_eq!(stack.charges_remaining(), Some(7));
    }

    #[test]
    fn format_copper_collapses_zero_parts() {
        assert_eq!(format_copper(0), "0c");
        assert_eq!(format_copper(4), "4c");
        assert_eq!(format_copper(COPPER_PER_SILVER), "1s");
        assert_eq!(format_copper(COPPER_PER_GOLD), "1g");
        assert_eq!(
            format_copper(COPPER_PER_GOLD + COPPER_PER_SILVER + 2),
            "1g 1s 2c"
        );
    }

    #[test]
    fn vendor_price_for_buyer_at_known_ranks() {
        // 0 ranks → no change.
        assert_eq!(vendor_price_for(0, 100, TradeSide::PlayerBuys), 100);
        assert_eq!(persuasion_modifier_pct(0, TradeSide::PlayerBuys), 0);
        // 5 ranks → -10%.
        assert_eq!(vendor_price_for(5, 100, TradeSide::PlayerBuys), 90);
        assert_eq!(persuasion_modifier_pct(5, TradeSide::PlayerBuys), -10);
        // 10 ranks → -20% (boundary).
        assert_eq!(vendor_price_for(10, 100, TradeSide::PlayerBuys), 80);
        assert_eq!(persuasion_modifier_pct(10, TradeSide::PlayerBuys), -20);
        // 15 ranks → still -20% (clamp).
        assert_eq!(vendor_price_for(15, 100, TradeSide::PlayerBuys), 80);
        assert_eq!(persuasion_modifier_pct(15, TradeSide::PlayerBuys), -20);
    }

    #[test]
    fn vendor_price_for_seller_inverts_sign() {
        assert_eq!(vendor_price_for(0, 100, TradeSide::PlayerSells), 100);
        assert_eq!(vendor_price_for(5, 100, TradeSide::PlayerSells), 110);
        assert_eq!(vendor_price_for(10, 100, TradeSide::PlayerSells), 120);
        assert_eq!(vendor_price_for(15, 100, TradeSide::PlayerSells), 120);
        assert_eq!(persuasion_modifier_pct(5, TradeSide::PlayerSells), 10);
        assert_eq!(persuasion_modifier_pct(10, TradeSide::PlayerSells), 20);
    }

    #[test]
    fn vendor_price_for_handles_small_amounts() {
        // 4-copper apple at 5 ranks: 10% off 4 = floor(4 * 10 / 100) = 0
        // (integer floor) → price stays 4. Sanity check that no overflow.
        assert_eq!(vendor_price_for(5, 4, TradeSide::PlayerBuys), 4);
        // 4-copper apple at 10 ranks: floor(4 * 20 / 100) = 0 still.
        // The next-cheapest discount tier kicks in at base >= 5c.
        assert_eq!(vendor_price_for(10, 5, TradeSide::PlayerBuys), 4);
    }
}

#[cfg(test)]
mod sell_tests {
    use super::*;

    fn offer(type_id: &str, quantity: u32) -> TradeOfferEntry {
        TradeOfferEntry {
            source: OfferSource::PlayerSlot(ItemSlotRef::Backpack(0)),
            type_id: type_id.to_owned(),
            properties: ObjectProperties::new(),
            quantity,
        }
    }

    #[test]
    fn coins_credit_at_face_value() {
        let definitions = OverworldObjectDefinitions::load_from_disk();
        assert_eq!(
            offer_credit_copper(&offer(COPPER_TYPE_ID, 7), &definitions, 0),
            7
        );
        assert_eq!(
            offer_credit_copper(&offer(SILVER_TYPE_ID, 2), &definitions, 0),
            2 * COPPER_PER_SILVER
        );
        assert_eq!(
            offer_credit_copper(&offer(GOLD_TYPE_ID, 1), &definitions, 0),
            COPPER_PER_GOLD
        );
    }

    #[test]
    fn goods_credit_at_half_market_value() {
        let definitions = OverworldObjectDefinitions::load_from_disk();
        let pelt = definitions.get("wolf_pelt").expect("wolf_pelt exists");
        assert_eq!(pelt.value_copper, Some(16));
        // One pelt: 16c on the shelf, 8c in the hand.
        assert_eq!(
            offer_credit_copper(&offer("wolf_pelt", 1), &definitions, 0),
            8
        );
        // Priced per stack, so three pelts are worth three times one.
        assert_eq!(
            offer_credit_copper(&offer("wolf_pelt", 3), &definitions, 0),
            24
        );
    }

    /// The reason the halving is applied to the stack rather than per item:
    /// a 2c trinket would otherwise round to 1c each, and a 1c one to nothing.
    #[test]
    fn cheap_trinkets_are_not_rounded_away() {
        let definitions = OverworldObjectDefinitions::load_from_disk();
        let tail = definitions.get("rat_tail").expect("rat_tail exists");
        assert_eq!(tail.value_copper, Some(2));
        assert_eq!(tail.sell_value_copper(1), 1);
        assert_eq!(tail.sell_value_copper(9), 9);
        assert_eq!(
            offer_credit_copper(&offer("rat_tail", 9), &definitions, 0),
            9
        );
    }

    #[test]
    fn persuasion_pays_the_seller_more() {
        let definitions = OverworldObjectDefinitions::load_from_disk();
        let plain = offer_credit_copper(&offer("grave_ring", 1), &definitions, 0);
        let smooth = offer_credit_copper(&offer("grave_ring", 1), &definitions, 10);
        assert_eq!(plain, 120); // 240c ring, sold at half
        assert_eq!(smooth, 144); // +20%, the clamp ceiling
                                 // Persuasion never *reduces* what the seller is paid.
        assert!(offer_credit_copper(&offer("grave_ring", 1), &definitions, 3) >= plain);
    }

    #[test]
    fn valueless_and_unknown_items_credit_nothing() {
        let definitions = OverworldObjectDefinitions::load_from_disk();
        // Quest items deliberately carry no `value_copper` so they cannot be
        // sold away by accident.
        assert_eq!(
            offer_credit_copper(&offer("iron_key", 1), &definitions, 0),
            0
        );
        assert_eq!(
            offer_credit_copper(&offer("no_such_item", 4), &definitions, 0),
            0
        );
    }

    /// Coins must stay unvalued: a market value on a coin would let a player
    /// sell 1 gold for its "half value" in change and launder the difference.
    #[test]
    fn coins_carry_no_market_value() {
        let definitions = OverworldObjectDefinitions::load_from_disk();
        for id in [COPPER_TYPE_ID, SILVER_TYPE_ID, GOLD_TYPE_ID] {
            let def = definitions.get(id).expect("coin definition exists");
            assert_eq!(def.value_copper, None, "{id} must not declare value_copper");
        }
    }

    #[test]
    fn restock_adds_to_finite_stock_only() {
        let mut finite = StockEntry {
            type_id: "wolf_pelt".to_owned(),
            price_copper: 16,
            stock: StockMode::Finite(2),
        };
        finite.restock(3);
        assert!(matches!(finite.stock, StockMode::Finite(5)));

        let mut infinite = StockEntry {
            type_id: "apple".to_owned(),
            price_copper: 4,
            stock: StockMode::Infinite,
        };
        infinite.restock(3);
        assert!(matches!(infinite.stock, StockMode::Infinite));
    }
}

/// End-to-end purchases through the real command pipeline: the merchant
/// settles whatever the player's column doesn't cover out of the purse.
#[cfg(all(test, feature = "server-sim"))]
mod purchase_tests {
    use super::*;
    use crate::game::commands::GameCommand;
    use crate::player::components::{ChatLog, Inventory, InventoryStack};
    use crate::world::components::SpaceResident;
    use crate::world::WorldConfig;
    use bevy::prelude::{App, Entity};

    const APPLE_PRICE: u32 = 10;

    /// Player at (10,10) with `purse` coin stacks, a shopkeeper next door
    /// selling apples at `APPLE_PRICE`.
    fn setup(purse: &[(&str, u32)]) -> (App, Entity) {
        let mut app = crate::test_support::TestServerApp::new().build();
        let player = crate::test_support::spawn_server_player(&mut app, 1, 10, 10);
        {
            let mut inventory = app.world_mut().get_mut::<Inventory>(player).unwrap();
            for (index, (type_id, quantity)) in purse.iter().enumerate() {
                inventory.backpack_slots[index] = Some(InventoryStack::item(
                    (*type_id).to_owned(),
                    ObjectProperties::new(),
                    *quantity,
                ));
            }
        }
        let space_id = app.world().resource::<WorldConfig>().current_space_id;
        let object_id = app
            .world_mut()
            .resource_mut::<crate::world::object_registry::ObjectRegistry>()
            .allocate_runtime_id("shopkeeper");
        app.world_mut().spawn((
            OverworldObject {
                object_id,
                definition_id: "shopkeeper".to_owned(),
                placement_seq: 0,
            },
            SpaceResident { space_id },
            TilePosition::ground(11, 10),
            Shopkeeper,
            Stockpile {
                wares: vec![StockEntry {
                    type_id: "apple".to_owned(),
                    price_copper: APPLE_PRICE,
                    stock: StockMode::Infinite,
                }],
            },
        ));

        for command in [
            GameCommand::InitiateTrade {
                target: TradeTarget::Shopkeeper { object_id },
            },
            GameCommand::BrowseShopBuy {
                session_id: 1,
                ware_index: 0,
                quantity: 1,
            },
        ] {
            crate::test_support::push_player_command(&mut app, 1, command);
            app.update();
        }
        (app, player)
    }

    fn confirm(app: &mut App) {
        for command in [
            GameCommand::ToggleTradeReady { session_id: 1 },
            GameCommand::ConfirmTrade { session_id: 1 },
        ] {
            crate::test_support::push_player_command(app, 1, command);
            app.update();
        }
    }

    fn purse_of(app: &App, player: Entity) -> u32 {
        crate::game::currency::purse_total_copper(app.world().get::<Inventory>(player).unwrap())
    }

    fn apples(app: &App, player: Entity) -> u32 {
        app.world()
            .get::<Inventory>(player)
            .unwrap()
            .backpack_slots
            .iter()
            .flatten()
            .filter(|stack| stack.type_id == "apple")
            .map(|stack| stack.quantity)
            .sum()
    }

    #[test]
    fn an_empty_offer_column_is_paid_straight_out_of_the_purse() {
        let (mut app, player) = setup(&[(SILVER_TYPE_ID, 1)]);
        confirm(&mut app);

        assert_eq!(apples(&app, player), 1, "the apple must change hands");
        assert_eq!(
            purse_of(&app, player),
            COPPER_PER_SILVER - APPLE_PRICE,
            "exactly the asking price leaves the purse"
        );
        assert!(
            app.world().resource::<ActiveTrades>().sessions.is_empty(),
            "a completed trade closes its session"
        );
    }

    #[test]
    fn offered_coin_is_credited_once_and_the_purse_covers_the_rest() {
        // 4c offered + 12c left in the purse; the apple costs 10c. If the
        // offered stack were counted twice the purse would end at 10c.
        let (mut app, player) = setup(&[(COPPER_TYPE_ID, 4), (SILVER_TYPE_ID, 1)]);
        crate::test_support::push_player_command(
            &mut app,
            1,
            GameCommand::OfferTradeItem {
                session_id: 1,
                source: ItemSlotRef::Backpack(0),
                quantity: 4,
            },
        );
        app.update();
        confirm(&mut app);

        assert_eq!(apples(&app, player), 1);
        assert_eq!(
            purse_of(&app, player),
            COPPER_PER_SILVER + 4 - APPLE_PRICE,
            "the offered coin must not be charged twice"
        );
    }

    #[test]
    fn a_short_purse_refuses_without_closing_the_session() {
        let (mut app, player) = setup(&[(COPPER_TYPE_ID, APPLE_PRICE - 1)]);
        confirm(&mut app);

        assert_eq!(apples(&app, player), 0, "no goods on a refused purchase");
        assert_eq!(
            purse_of(&app, player),
            APPLE_PRICE - 1,
            "a refused purchase must not touch the purse"
        );
        assert!(
            app.world()
                .resource::<ActiveTrades>()
                .sessions
                .contains_key(&1),
            "the session must survive so the basket isn't lost"
        );
        let chat_log = app.world().get::<ChatLog>(player).unwrap();
        assert!(
            chat_log.lines.iter().any(|line| line.contains("Short by")),
            "expected the shortfall line; got {:?}",
            chat_log.lines
        );
    }
}
