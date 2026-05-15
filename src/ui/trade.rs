//! Trade popup UI: reads `ClientGameState.current_trade` and drives the
//! floating trade window. The window is a `MovableWindow` — spawned by
//! `sync_trade_window_lifecycle` when a trade session opens and despawned
//! when it closes. The body is laid out in three side-by-side columns —
//! Merchant (left, drag source for wares), Us (middle, the player's offer),
//! and Them (right, the partner's offer). Interaction is drag-and-drop
//! only: drag a merchant ware row onto the Them column to buy; drag a
//! docked backpack/equipment slot onto the Us column to offer; drag a Us
//! row out of the popup to withdraw. The actual drag mechanics for slots
//! are handled by `handle_movable_dragging` in `ui::systems`.

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::game::commands::GameCommand;
use crate::game::resources::{ClientGameState, PendingGameCommands};
use crate::game::trade::{ClientTradeView, TradeOfferEntry, TradeSessionId, WareView};
use crate::ui::components::{
    ItemSlotButton, ItemSlotKind, TradeButtonLabel, TradeCancelButton, TradeColumn,
    TradeConfirmButton, TradePartnerLabel, TradePopupCloseButton, TradePopupRoot, TradeReadyButton,
    TradeSlotButton,
};
use crate::ui::movable_window::{
    spawn_movable_window, spawn_themed_close_button, val_to_px, MovableWindowDrag, MovableWindowId,
};
use crate::ui::resources::TradePopupState;
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton};
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Caches the most-recently-rendered signature per column so the children
/// rebuild only fires when the column's content actually changes.
///
/// `session_id` pins the cache to a specific trade session — when the
/// session closes (or a new one starts), the column entities have been
/// despawned and respawned with empty children, so we wipe the cached
/// signatures to force a fresh build. Without this, re-opening a trade
/// with the same partner+wares leaves the columns blank (cache hits
/// against stale state from the previous session's entities).
#[derive(Resource, Default)]
pub struct TradePanelRenderState {
    pub session_id: Option<TradeSessionId>,
    pub merchant: Option<String>,
    pub us: Option<String>,
    pub them: Option<String>,
}

fn merchant_signature(wares: Option<&[WareView]>) -> String {
    let mut s = String::new();
    if let Some(wares) = wares {
        for w in wares {
            s.push_str(&w.type_id);
            s.push('@');
            s.push_str(&w.price_copper.to_string());
            s.push(':');
            match w.stock_remaining {
                Some(n) => s.push_str(&n.to_string()),
                None => s.push_str("inf"),
            }
            s.push(';');
        }
    }
    s
}

fn offers_signature(offers: &[TradeOfferEntry]) -> String {
    let mut s = String::new();
    for entry in offers {
        s.push_str(&entry.type_id);
        s.push('x');
        s.push_str(&entry.quantity.to_string());
        s.push(';');
    }
    s
}

pub fn sync_trade_panel_partner_label(
    client_state: Res<ClientGameState>,
    mut label_query: Query<&mut Text, With<TradePartnerLabel>>,
) {
    let Ok(mut label) = label_query.single_mut() else {
        return;
    };
    let new_text = match client_state.current_trade.as_ref() {
        Some(view) => format!(
            "Trading with {}  (us {} / them {})",
            view.partner_name,
            yes_no(view.our_ready),
            yes_no(view.their_ready),
        ),
        None => "No active trade.".to_owned(),
    };
    if label.0 != new_text {
        label.0 = new_text;
    }
}

fn yes_no(flag: bool) -> &'static str {
    if flag {
        "ready"
    } else {
        "wait"
    }
}

pub fn sync_trade_panel_buttons(
    client_state: Res<ClientGameState>,
    mut label_query: Query<(&mut Text, &TradeButtonLabel)>,
) {
    let Some(view) = client_state.current_trade.as_ref() else {
        return;
    };
    for (mut text, label_kind) in &mut label_query {
        let new = match label_kind {
            TradeButtonLabel::Ready => {
                if view.our_ready {
                    "Unready"
                } else {
                    "Ready"
                }
            }
            TradeButtonLabel::Confirm => {
                if view.our_confirmed {
                    "Confirmed"
                } else if view.both_ready_or_else() {
                    "Confirm"
                } else {
                    "Confirm (locked)"
                }
            }
            TradeButtonLabel::Cancel => continue,
        };
        if text.0 != new {
            text.0 = new.to_owned();
        }
    }
}

/// Spawn / despawn the trade window based on `TradePopupState.session_id`.
/// On open we use the cached position/size from the last session (default
/// to centered on screen, `DEFAULT_SIZE`). On close we read the current
/// position/size off the entity's `Node` so the next session re-opens in
/// the same place.
#[allow(clippy::too_many_arguments)]
pub fn sync_trade_window_lifecycle(
    mut commands: Commands,
    mut state: ResMut<TradePopupState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    existing: Query<(Entity, &Node), With<TradePopupRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let want_open = state.session_id.is_some();
    let existing_root = existing.iter().next();

    match (want_open, existing_root) {
        (true, None) => {
            let win = window_query
                .single()
                .map(|window| Vec2::new(window.width(), window.height()))
                .unwrap_or(Vec2::new(1280.0, 720.0));
            let size = state.last_size.unwrap_or(TradePopupState::DEFAULT_SIZE);
            let pos = state
                .last_position
                .unwrap_or_else(|| ((win - size) * 0.5).max(Vec2::ZERO));
            let root = spawn_trade_window(&mut commands, &theme, &palette, pos, size);
            drag.focused = Some(root);
        }
        (false, Some((root, _))) => {
            commands.entity(root).despawn();
            if drag.focused == Some(root) {
                drag.focused = None;
            }
            if drag.dragging.is_some_and(|(e, _)| e == root) {
                drag.dragging = None;
            }
        }
        (true, Some((_, node))) => {
            // Cache position/size each frame so an external despawn (e.g.
            // the partner cancels and the server clears `session_id`) still
            // remembers where the user had the window.
            let pos = Vec2::new(val_to_px(node.left), val_to_px(node.top));
            let size = Vec2::new(val_to_px(node.width), val_to_px(node.height));
            if state.last_position != Some(pos) {
                state.last_position = Some(pos);
            }
            if state.last_size != Some(size) {
                state.last_size = Some(size);
            }
        }
        (false, None) => {}
    }
}

fn spawn_trade_window(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    position: Vec2,
    size: Vec2,
) -> Entity {
    let spawned = spawn_movable_window(
        commands,
        theme,
        palette,
        MovableWindowId::Trade,
        "Trade",
        size,
        position,
        crate::ui::resources::TradePopupState::MIN_SIZE,
    );

    commands
        .entity(spawned.root)
        .insert((TradePopupRoot, crate::ui::components::HudRoot));

    // Trade has its own close button that emits `CancelTrade` rather than
    // despawning the entity directly — the despawn happens via the
    // lifecycle once the server confirms the trade ended.
    commands.entity(spawned.title_bar).with_children(|bar| {
        spawn_themed_close_button(bar, theme, TradePopupCloseButton);
    });

    commands.entity(spawned.body).with_children(|body| {
        body.spawn((
            Text::new("Trading with: ..."),
            TradePartnerLabel,
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(palette.text_muted),
            Node {
                width: percent(100.0),
                ..default()
            },
        ));

        body.spawn((
            Node {
                width: percent(100.0),
                flex_grow: 1.0,
                column_gap: px(8.0),
                min_height: px(0.0),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|columns| {
            crate::ui::setup::spawn_trade_column(
                columns,
                palette,
                "Merchant",
                TradeColumn::Merchant,
            );
            crate::ui::setup::spawn_trade_column(columns, palette, "Them", TradeColumn::Them);
            crate::ui::setup::spawn_trade_column(columns, palette, "Us", TradeColumn::Us);
        });
    });

    commands.entity(spawned.root).with_children(|root| {
        root.spawn((
            Node {
                width: percent(100.0),
                column_gap: px(6.0),
                padding: UiRect::axes(px(10.0), px(8.0)),
                border: UiRect::top(px(1.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(palette.surface_raised),
            BorderColor::all(palette.border_slot),
        ))
        .with_children(|footer| {
            crate::ui::setup::spawn_trade_button(
                footer,
                theme,
                palette,
                "Ready",
                TradeButtonLabel::Ready,
                TradeReadyButton,
                ButtonStyle::Primary,
            );
            crate::ui::setup::spawn_trade_button(
                footer,
                theme,
                palette,
                "Confirm",
                TradeButtonLabel::Confirm,
                TradeConfirmButton,
                ButtonStyle::Primary,
            );
            crate::ui::setup::spawn_trade_button(
                footer,
                theme,
                palette,
                "Cancel",
                TradeButtonLabel::Cancel,
                TradeCancelButton,
                ButtonStyle::Danger,
            );
        });
    });

    spawned.root
}

pub fn handle_trade_popup_close_click(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    state: Res<TradePopupState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    button_query: Query<(&ComputedNode, &UiGlobalTransform), With<TradePopupCloseButton>>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(session_id) = state.session_id else {
        return;
    };
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((node, transform)) = button_query.single() else {
        return;
    };
    if point_in_ui_node(cursor, node, transform) {
        pending_commands.push(GameCommand::CancelTrade { session_id });
    }
}

/// Rebuild the three trade columns whenever the trade snapshot changes.
/// Each column owns its own signature so an offer-side change does not
/// force the merchant column to redraw (and vice versa).
pub fn sync_trade_panel_rows(
    mut render_state: ResMut<TradePanelRenderState>,
    client_state: Res<ClientGameState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    definitions: Res<OverworldObjectDefinitions>,
    column_query: Query<(Entity, &TradeColumn)>,
    mut commands: Commands,
) {
    let view = client_state.current_trade.as_ref();
    let current_session_id = view.map(|v| v.session_id);
    if render_state.session_id != current_session_id {
        render_state.session_id = current_session_id;
        render_state.merchant = None;
        render_state.us = None;
        render_state.them = None;
    }
    let (merchant_sig, us_sig, them_sig) = match view {
        Some(view) => (
            merchant_signature(view.wares.as_deref()),
            offers_signature(&view.our_offers),
            offers_signature(&view.their_offers),
        ),
        None => (String::new(), String::new(), String::new()),
    };

    for (entity, column) in &column_query {
        let (cached, current, items): (&mut Option<String>, &str, ColumnContent<'_>) = match column
        {
            TradeColumn::Merchant => (
                &mut render_state.merchant,
                merchant_sig.as_str(),
                ColumnContent::Merchant(view.and_then(|v| v.wares.as_deref()).unwrap_or(&[])),
            ),
            TradeColumn::Us => (
                &mut render_state.us,
                us_sig.as_str(),
                ColumnContent::Offers(view.map(|v| v.our_offers.as_slice()).unwrap_or(&[])),
            ),
            TradeColumn::Them => (
                &mut render_state.them,
                them_sig.as_str(),
                ColumnContent::Offers(view.map(|v| v.their_offers.as_slice()).unwrap_or(&[])),
            ),
        };

        if cached.as_deref() == Some(current) {
            continue;
        }
        *cached = Some(current.to_owned());

        commands.entity(entity).despawn_related::<Children>();
        let labels = items.into_row_labels(&definitions);
        let theme = theme.clone();
        let palette = *palette;
        let column = *column;
        commands.entity(entity).with_children(move |parent| {
            for (index, label) in labels.iter().enumerate() {
                spawn_trade_row(parent, &theme, &palette, column, index, label);
            }
            match column {
                // Us and Them get an always-on drop-zone slot beneath their
                // populated rows; it expands (`flex_grow: 1.0`) to fill the
                // rest of the column so drops into the empty space below
                // still register as a TradeUs/TradeThem hit.
                TradeColumn::Us | TradeColumn::Them => {
                    spawn_trade_drop_zone(parent, &palette, column, labels.is_empty());
                }
                TradeColumn::Merchant => {
                    if labels.is_empty() {
                        parent.spawn((
                            Text::new("(no wares)"),
                            TextFont {
                                font_size: 12.0,
                                ..default()
                            },
                            TextColor(palette.text_muted),
                        ));
                    }
                }
            }
        });
    }
}

fn spawn_trade_drop_zone(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    column: TradeColumn,
    show_hint: bool,
) {
    // Sentinel index used by the drag release handler — only the variant
    // matters there, not the index.
    let kind = match column {
        TradeColumn::Us => ItemSlotKind::TradeUs { index: usize::MAX },
        TradeColumn::Them => ItemSlotKind::TradeThem { index: usize::MAX },
        TradeColumn::Merchant => return,
    };
    parent
        .spawn((
            Button,
            ItemSlotButton { kind },
            TradeSlotButton,
            Node {
                width: percent(100.0),
                flex_grow: 1.0,
                min_height: px(40.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|button| {
            if show_hint {
                let hint = match column {
                    TradeColumn::Us => "(drop items here)",
                    TradeColumn::Them => "(drop wares here)",
                    TradeColumn::Merchant => "",
                };
                button.spawn((
                    Text::new(hint),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(palette.text_muted),
                ));
            }
        });
}

enum ColumnContent<'a> {
    Merchant(&'a [WareView]),
    Offers(&'a [TradeOfferEntry]),
}

impl<'a> ColumnContent<'a> {
    fn into_row_labels(self, definitions: &OverworldObjectDefinitions) -> Vec<String> {
        match self {
            ColumnContent::Merchant(wares) => wares
                .iter()
                .map(|ware| format_ware_label(ware, definitions))
                .collect(),
            ColumnContent::Offers(offers) => offers
                .iter()
                .map(|entry| format_offer_label(entry, definitions))
                .collect(),
        }
    }
}

fn format_ware_label(ware: &WareView, definitions: &OverworldObjectDefinitions) -> String {
    let display = definitions
        .get(&ware.type_id)
        .map(|def| def.name.clone())
        .unwrap_or_else(|| ware.display_name.clone());
    let (g, s, c) = crate::game::currency::split(ware.price_copper);
    let mut price = String::new();
    if g > 0 {
        price.push_str(&format!("{}g ", g));
    }
    if s > 0 {
        price.push_str(&format!("{}s ", s));
    }
    if c > 0 || (g == 0 && s == 0) {
        price.push_str(&format!("{}c", c));
    }
    let stock = match ware.stock_remaining {
        Some(n) => format!("  [{}]", n),
        None => String::new(),
    };
    format!("{}  {}{}", display, price, stock)
}

fn format_offer_label(entry: &TradeOfferEntry, definitions: &OverworldObjectDefinitions) -> String {
    let display = definitions
        .get(&entry.type_id)
        .map(|def| def.name.clone())
        .unwrap_or_else(|| entry.type_id.clone());
    if entry.quantity > 1 {
        format!("{} x{}", display, entry.quantity)
    } else {
        display
    }
}

fn spawn_trade_row(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    column: TradeColumn,
    index: usize,
    label: &str,
) {
    let kind = match column {
        TradeColumn::Merchant => ItemSlotKind::MerchantWare { ware_index: index },
        TradeColumn::Us => ItemSlotKind::TradeUs { index },
        TradeColumn::Them => ItemSlotKind::TradeThem { index },
    };
    let (bg, border, text_color) = idle_colors(palette, ButtonStyle::Slot, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Slot),
            ItemSlotButton { kind },
            TradeSlotButton,
            Node {
                width: percent(100.0),
                min_height: px(22.0),
                padding: UiRect::axes(px(6.0), px(2.0)),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            ImageNode::new(theme.button_frame.clone())
                .with_mode(theme.button_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(text_color),
            ));
        });
}

#[allow(clippy::too_many_arguments)]
pub fn handle_trade_panel_clicks(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    client_state: Res<ClientGameState>,
    state: Res<TradePopupState>,
    mut pending_commands: ResMut<PendingGameCommands>,
    ready_query: Query<(&ComputedNode, &UiGlobalTransform), With<TradeReadyButton>>,
    confirm_query: Query<(&ComputedNode, &UiGlobalTransform), With<TradeConfirmButton>>,
    cancel_query: Query<(&ComputedNode, &UiGlobalTransform), With<TradeCancelButton>>,
) {
    if !mouse_input.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(session_id) = state.session_id else {
        return;
    };
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    let view = client_state.current_trade.as_ref();

    if let Ok((node, transform)) = ready_query.single() {
        if point_in_ui_node(cursor_position, node, transform) {
            pending_commands.push(GameCommand::ToggleTradeReady { session_id });
            return;
        }
    }

    if let Ok((node, transform)) = confirm_query.single() {
        if point_in_ui_node(cursor_position, node, transform) {
            if view.is_some_and(|v| v.our_ready && v.their_ready) {
                pending_commands.push(GameCommand::ConfirmTrade { session_id });
            }
            return;
        }
    }

    if let Ok((node, transform)) = cancel_query.single() {
        if point_in_ui_node(cursor_position, node, transform) {
            pending_commands.push(GameCommand::CancelTrade { session_id });
        }
    }
}

pub(crate) fn point_in_ui_node(
    point: Vec2,
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
) -> bool {
    // `point` comes from `Window::cursor_position()` in logical pixels, but
    // `ComputedNode` / `UiGlobalTransform` are in physical pixels. On HiDPI
    // displays (scale_factor > 1) we must scale up or the hit-test misses.
    let inv = computed.inverse_scale_factor();
    let physical_point = if inv > 0.0 { point / inv } else { point };
    computed.contains_point(*transform, physical_point)
}

trait ClientTradeViewExt {
    fn both_ready_or_else(&self) -> bool;
}

impl ClientTradeViewExt for ClientTradeView {
    fn both_ready_or_else(&self) -> bool {
        self.our_ready && self.their_ready
    }
}
