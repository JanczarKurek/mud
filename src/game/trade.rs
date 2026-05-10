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

use crate::combat::components::CombatTarget;
use crate::game::commands::{GameCommand, ItemSlotRef};
use crate::game::currency::{
    COPPER_PER_GOLD, COPPER_PER_SILVER, COPPER_TYPE_ID, GOLD_TYPE_ID, SILVER_TYPE_ID,
};
use crate::game::helpers::is_near_player;
use crate::game::resources::{
    ChatLogState, GameUiEvent, InventoryState, PendingGameCommands, PendingGameUiEvents,
};
use crate::game::shop::{Shopkeeper, StockMode, Stockpile};
use crate::player::components::{
    InventoryStack, MaxCarryWeight, MovementCooldown, Player, PlayerId, PlayerIdentity, VitalStats,
};
use crate::world::components::{OverworldObject, SpaceResident, TilePosition};
use crate::world::map_layout::ObjectProperties;
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
}

/// Per-ware projection used by the trade panel's Browse Wares list.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WareView {
    pub type_id: String,
    pub display_name: String,
    pub price_copper: u32,
    /// `None` for infinite stock; `Some(n)` for finite remaining.
    pub stock_remaining: Option<u32>,
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

impl ActiveTrades {
    pub fn allocate_session_id(&mut self) -> TradeSessionId {
        self.next_id += 1;
        self.next_id
    }

    /// Find the active session containing `player_id` (each player can be in
    /// at most one trade) plus which side they sit on.
    pub fn find_for_player(&self, player_id: PlayerId) -> Option<(TradeSessionId, Side)> {
        self.sessions.iter().find_map(|(id, session)| match session.participants {
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

impl TradeSession {
    /// Project this session for `viewing_player`'s perspective. Returns `None`
    /// if the player is not in this session.
    pub fn project_for(
        &self,
        viewing_player: PlayerId,
        partner_name: String,
        partner_kind: TradePartnerKind,
        wares: Option<Vec<WareView>>,
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
pub fn cleanup_invalid_trades(
    mut active_trades: ResMut<ActiveTrades>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    player_position_query: Query<
        (&PlayerIdentity, &SpaceResident, &TilePosition),
        With<Player>,
    >,
    shopkeeper_query: Query<
        (&OverworldObject, &SpaceResident, &TilePosition),
        (With<Shopkeeper>, Without<Player>),
    >,
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
                        if space_a != space_b || !is_near_player(&tile_a, &tile_b) {
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
                        if space_p != space_s || !is_near_player(&tile_p, &tile_s) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::shop::StockEntry;

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
}

/// Drains all `Trade*` `GameCommand` variants from `PendingGameCommands` and
/// applies them to `ActiveTrades` + the involved players' inventories. Mirrors
/// the `process_dialog_commands` / `process_rotate_commands` pattern: scheduled
/// in `CommandIntercept` so the variants never reach `process_game_commands`.
#[allow(clippy::too_many_arguments)]
pub fn process_trade_commands(
    mut pending_commands: ResMut<PendingGameCommands>,
    mut active_trades: ResMut<ActiveTrades>,
    mut ui_events: ResMut<PendingGameUiEvents>,
    definitions: Res<OverworldObjectDefinitions>,
    mut player_queries: ParamSet<(
        Query<
            (&PlayerIdentity, &SpaceResident, &TilePosition, &OverworldObject),
            With<Player>,
        >,
        Query<
            (
                Entity,
                &PlayerIdentity,
                &mut InventoryState,
                &mut ChatLogState,
                &mut SpaceResident,
                &mut TilePosition,
                &mut MovementCooldown,
                &mut VitalStats,
                Option<&CombatTarget>,
            ),
            With<Player>,
        >,
    )>,
    max_carry_query: Query<&MaxCarryWeight, With<Player>>,
    shopkeeper_query: Query<
        (&OverworldObject, &SpaceResident, &TilePosition),
        (With<Shopkeeper>, Without<Player>),
    >,
    mut stockpile_query: Query<(&OverworldObject, &mut Stockpile)>,
) {
    let drained: Vec<_> = pending_commands.commands.drain(..).collect();
    let mut remaining = Vec::with_capacity(drained.len());

    for queued in drained {
        let acting_player_id = match queued.player_id {
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

        match queued.command {
            GameCommand::InitiateTrade { target } => {
                handle_initiate_trade(
                    acting_player_id,
                    target,
                    &mut active_trades,
                    &mut ui_events,
                    &player_queries.p0(),
                    &shopkeeper_query,
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
                    &definitions,
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
            other => remaining.push(crate::game::resources::QueuedGameCommand {
                player_id: queued.player_id,
                command: other,
            }),
        }
    }

    pending_commands.commands = remaining;
}

fn handle_initiate_trade(
    acting_player_id: PlayerId,
    target: TradeTarget,
    active_trades: &mut ActiveTrades,
    ui_events: &mut PendingGameUiEvents,
    player_position_query: &Query<
        (&PlayerIdentity, &SpaceResident, &TilePosition, &OverworldObject),
        With<Player>,
    >,
    shopkeeper_query: &Query<
        (&OverworldObject, &SpaceResident, &TilePosition),
        (With<Shopkeeper>, Without<Player>),
    >,
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
            let target = player_position_query.iter().find(|(_, resident, _, object)| {
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

            if !is_near_player(&acting_tile, target_tile) {
                bevy::log::debug!("InitiateTrade rejected: target out of range");
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
            let shopkeeper = shopkeeper_query
                .iter()
                .find(|(object, resident, _)| {
                    resident.space_id == acting_space && object.object_id == shop_object_id
                });
            let Some((_, _, shop_tile)) = shopkeeper else {
                bevy::log::debug!(
                    "InitiateTrade: target object {shop_object_id} is not a shopkeeper"
                );
                return;
            };
            if !is_near_player(&acting_tile, shop_tile) {
                bevy::log::debug!("InitiateTrade rejected: shopkeeper out of range");
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

fn handle_offer_trade_item(
    acting_player_id: PlayerId,
    session_id: TradeSessionId,
    source: ItemSlotRef,
    quantity: u32,
    active_trades: &mut ActiveTrades,
    definitions: &OverworldObjectDefinitions,
    player_inventory_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
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
    let Some((type_id, properties, available)) =
        read_player_slot(&source, inventory, definitions)
    else {
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

#[allow(clippy::too_many_arguments)]
fn handle_confirm_trade(
    acting_player_id: PlayerId,
    session_id: TradeSessionId,
    active_trades: &mut ActiveTrades,
    ui_events: &mut PendingGameUiEvents,
    definitions: &OverworldObjectDefinitions,
    player_inventory_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    max_carry_query: &Query<&MaxCarryWeight, With<Player>>,
    stockpile_query: &mut Query<(&OverworldObject, &mut Stockpile)>,
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

    let (success, players_to_notify): (bool, Vec<PlayerId>) = match session_snapshot.participants {
        TradeParticipants::PlayerToPlayer { a, b } => {
            let ok = commit_player_to_player_trade(
                &session_snapshot,
                a,
                b,
                definitions,
                player_inventory_query,
                max_carry_query,
            );
            (ok, vec![a, b])
        }
        TradeParticipants::PlayerToShop {
            player,
            shop_object_id,
        } => {
            let ok = commit_player_to_shop_trade(
                &session_snapshot,
                player,
                shop_object_id,
                definitions,
                player_inventory_query,
                max_carry_query,
                stockpile_query,
            );
            (ok, vec![player])
        }
    };

    active_trades.remove(session_id);

    let outcome = if success {
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
#[allow(clippy::too_many_arguments)]
fn handle_browse_shop_buy(
    acting_player_id: PlayerId,
    session_id: TradeSessionId,
    ware_index: usize,
    quantity: u32,
    active_trades: &mut ActiveTrades,
    _definitions: &OverworldObjectDefinitions,
    player_inventory_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
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
            OfferSource::Stockpile { ware_index: idx } if idx == ware_index => {
                Some(entry.quantity)
            }
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

#[allow(clippy::too_many_arguments)]
fn commit_player_to_shop_trade(
    session: &TradeSession,
    player: PlayerId,
    shop_object_id: u64,
    definitions: &OverworldObjectDefinitions,
    player_inventory_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
    max_carry_query: &Query<&MaxCarryWeight, With<Player>>,
    stockpile_query: &mut Query<(&OverworldObject, &mut Stockpile)>,
) -> bool {
    let entity = player_inventory_query
        .iter()
        .find(|(_, identity, _, _, _, _, _, _, _)| identity.id == player)
        .map(|(e, _, _, _, _, _, _, _, _)| e);
    let Some(entity) = entity else {
        return false;
    };
    let mut inv = match player_inventory_query.get(entity) {
        Ok((_, _, inventory, _, _, _, _, _, _)) => inventory.clone(),
        Err(_) => return false,
    };
    let max_carry = max_carry_query.get(entity).copied().unwrap_or_default();

    // Validate player coin offers against current inventory.
    if !validate_offers_against(&session.offers_a, &inv, definitions) {
        return false;
    }

    // Total price the merchant is asking for (sum of ware price * quantity).
    let mut total_owed_copper: u32 = 0;
    for offer in &session.offers_b {
        let OfferSource::Stockpile { ware_index } = &offer.source else {
            return false;
        };
        let stocks = stockpile_query
            .iter()
            .find(|(object, _)| object.object_id == shop_object_id);
        let Some((_, stockpile)) = stocks else {
            return false;
        };
        let Some(entry) = stockpile.wares.get(*ware_index) else {
            return false;
        };
        if entry.type_id != offer.type_id {
            return false;
        }
        if let StockMode::Finite(n) = entry.stock {
            if n < offer.quantity {
                return false;
            }
        }
        total_owed_copper = total_owed_copper
            .saturating_add(entry.price_copper.saturating_mul(offer.quantity));
    }

    // Sum the coin value the player is offering. Non-coin items in offers_a
    // are transferred to the merchant for free (the merchant doesn't price
    // them) — that's the player's choice for putting them there.
    let total_offered_copper: u32 = session
        .offers_a
        .iter()
        .map(|entry| match entry.type_id.as_str() {
            COPPER_TYPE_ID => entry.quantity,
            SILVER_TYPE_ID => entry.quantity.saturating_mul(COPPER_PER_SILVER),
            GOLD_TYPE_ID => entry.quantity.saturating_mul(COPPER_PER_GOLD),
            _ => 0,
        })
        .sum();

    if total_offered_copper < total_owed_copper {
        let shortfall = total_owed_copper - total_offered_copper;
        if let Ok((_, _, _, mut chat_log, _, _, _, _, _)) =
            player_inventory_query.get_mut(entity)
        {
            chat_log.push_narrator(&format!(
                "The merchant frowns. \"Short by {} — bring more coin.\"",
                format_copper(shortfall)
            ));
        }
        return false;
    }

    // Remove player's coin offers from the inventory snapshot.
    if !remove_offered_from(&session.offers_a, &mut inv) {
        return false;
    }

    // Insert the wares into the snapshot.
    if !insert_offers_into(&session.offers_b, &mut inv, definitions, &max_carry) {
        return false;
    }

    // Commit: write the inventory snapshot back and decrement finite stocks.
    if let Ok((_, _, mut inventory, mut chat_log, _, _, _, _, _)) =
        player_inventory_query.get_mut(entity)
    {
        *inventory = inv;
        chat_log.push_narrator("Trade complete.");
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
    }
    true
}

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
fn read_player_slot(
    slot: &ItemSlotRef,
    inventory: &InventoryState,
    definitions: &OverworldObjectDefinitions,
) -> Option<(String, ObjectProperties, u32)> {
    match slot {
        ItemSlotRef::Backpack(idx) => {
            let stack = inventory.backpack_slots.get(*idx)?.as_ref()?;
            Some((stack.type_id.clone(), stack.properties.clone(), stack.quantity))
        }
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
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => {
            let outer = inventory.backpack_slots.get(*backpack_slot)?.as_ref()?;
            let nested = outer.contained_slots.as_ref()?.get(*sub_slot)?.as_ref()?;
            Some((
                nested.type_id.clone(),
                nested.properties.clone(),
                nested.quantity,
            ))
        }
        ItemSlotRef::Container { .. } => {
            let _ = definitions;
            None
        }
    }
}

/// Atomically transfer all offered items between two players. Returns `true`
/// on success; on validation failure (source no longer resolves, weight cap,
/// or no inventory space) returns `false` and leaves both inventories
/// unchanged.
fn commit_player_to_player_trade(
    session: &TradeSession,
    player_a: PlayerId,
    player_b: PlayerId,
    definitions: &OverworldObjectDefinitions,
    player_inventory_query: &mut Query<
        (
            Entity,
            &PlayerIdentity,
            &mut InventoryState,
            &mut ChatLogState,
            &mut SpaceResident,
            &mut TilePosition,
            &mut MovementCooldown,
            &mut VitalStats,
            Option<&CombatTarget>,
        ),
        With<Player>,
    >,
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
    if !validate_offers_against(&session.offers_a, &inv_a, definitions) {
        return false;
    }
    if !validate_offers_against(&session.offers_b, &inv_b, definitions) {
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

fn validate_offers_against(
    offers: &[TradeOfferEntry],
    inventory: &InventoryState,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    // Group offers by source slot and ensure the slot still holds enough
    // matching items. Stockpile-sourced offers are validated separately at
    // commit time and skipped here.
    let mut required: HashMap<ItemSlotRef, u32> = HashMap::new();
    for offer in offers {
        let OfferSource::PlayerSlot(slot) = &offer.source else {
            continue;
        };
        *required.entry(*slot).or_insert(0) += offer.quantity;
        let Some((type_id, _properties, available)) = read_player_slot(slot, inventory, definitions)
        else {
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

fn remove_offered_from(
    offers: &[TradeOfferEntry],
    inventory: &mut InventoryState,
) -> bool {
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

fn decrement_player_slot(
    slot: &ItemSlotRef,
    amount: u32,
    inventory: &mut InventoryState,
) -> bool {
    match slot {
        ItemSlotRef::Backpack(idx) => {
            let Some(slot) = inventory.backpack_slots.get_mut(*idx) else {
                return false;
            };
            let Some(stack) = slot else {
                return false;
            };
            if stack.quantity < amount {
                return false;
            }
            stack.quantity -= amount;
            if stack.quantity == 0 {
                *slot = None;
            }
            true
        }
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
        ItemSlotRef::PouchInBackpack {
            backpack_slot,
            sub_slot,
        } => {
            let Some(outer) = inventory.backpack_slots.get_mut(*backpack_slot) else {
                return false;
            };
            let Some(outer_stack) = outer else {
                return false;
            };
            let Some(contained) = outer_stack.contained_slots.as_mut() else {
                return false;
            };
            let Some(inner_slot) = contained.get_mut(*sub_slot) else {
                return false;
            };
            let Some(inner_stack) = inner_slot else {
                return false;
            };
            if inner_stack.quantity < amount {
                return false;
            }
            inner_stack.quantity -= amount;
            if inner_stack.quantity == 0 {
                *inner_slot = None;
            }
            true
        }
        ItemSlotRef::Container { .. } => false,
    }
}

/// Insert each offer's items into `inventory`, merging into existing stacks
/// where possible and respecting weight caps. Returns `false` if any item
/// can't be placed (no free slot or hard-cap exceeded).
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
        let mut new_stack = InventoryStack::item(
            offer.type_id.clone(),
            offer.properties.clone(),
            take,
        );
        if let Some(capacity) = definition.container_capacity {
            new_stack.contained_slots = Some(vec![None; capacity]);
        }
        inventory.backpack_slots[empty_index] = Some(new_stack);
        current_weight += per_unit_weight * take as f32;
        remaining -= take;
    }

    true
}
