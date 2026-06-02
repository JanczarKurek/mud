use bevy::prelude::*;
use bevy::ui::widget::NodeImageMode;
use bevy_terminal::{spawn_terminal, LineStyle, TerminalConfig, TerminalInputConfig, TerminalLine};

use crate::ui::components::{
    BackpackPanelContent, BackpackPanelUndockButton, BackpackSlotRow, BottomHudColumn,
    BottomPanelHideButton, CarryWeightLabel, ChatAreaContainer, ChatPanel, ChatTerminal,
    ContainerPanelContent, ContainerPanelUndockButton, ContainerSlotButton, ContainerSlotImage,
    ContextMenuAttackButton, ContextMenuForceLockButton, ContextMenuHideButton,
    ContextMenuInspectButton, ContextMenuInteractButton, ContextMenuOfferToTradeButton,
    ContextMenuOpenButton, ContextMenuPickLockButton, ContextMenuRoot,
    ContextMenuTakePartialButton, ContextMenuTalkButton, ContextMenuTradeButton,
    ContextMenuUseButton, ContextMenuUseKeyButton, ContextMenuUseOnButton, DockedPanelBody,
    DockedPanelCanvas, DockedPanelCloseButton, DockedPanelDragHandle, DockedPanelResizeHandle,
    DockedPanelRoot, DockedPanelTitle, DragPreviewImage, DragPreviewLabel, DragPreviewQuantity,
    DragPreviewRoot, EquipmentPanelContent, EquipmentPanelUndockButton, EquipmentSlotButton,
    EquipmentSlotImage, EquipmentSlotLabel, ExperienceFill, ExperienceLabel, FloatingMinimapZoomInButton,
    FloatingMinimapZoomLabel, FloatingMinimapZoomOutButton, HealthFill, HealthLabel,
    HudMinimapZoomInButton, HudMinimapZoomLabel, HudMinimapZoomOutButton, HudRoot, ItemSlotButton,
    ItemSlotImage, ItemSlotKind, ItemSlotQuantityLabel, ItemTooltipLabel, ItemTooltipRoot,
    MagicEffectsLabel, ManaFill, ManaLabel, MinimapCanvas, MinimapMode, MinimapPanelUndockButton,
    MinimapView, NearbyNpcsList, NearbyNpcsPanelUndockButton, PythonConsolePanel,
    PythonConsoleRestartButton, PythonConsoleTerminal, RegenBuffLabel, RightSidebarRoot,
    StatusPanelContent, StatusPanelUndockButton, TakePartialAmountLabel, TakePartialCancelButton,
    TakePartialConfirmButton, TakePartialDecButton, TakePartialIncButton, TakePartialPopupRoot,
    TradeButtonLabel, TradeColumn,
};
use crate::ui::menu_bar::{spawn_menu_bar, MENU_BAR_HEIGHT};
use crate::ui::minimap::{make_minimap_image, FULL_MAP_BODY_SIZE, HUD_MINIMAP_SIZE};
use crate::ui::movable_window::{spawn_themed_close_button, spawn_themed_icon_button};
use crate::ui::resources::{DockedPanelState, HudMinimapSettings};
use crate::ui::retro_bar::{spawn_retro_bar, RetroBarStyle};
use crate::ui::theme::widgets::{
    idle_colors, spawn_themed_button, ButtonStyle, ThemedButton, ThemedPanel,
};
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::ui::{CHAT_TERMINAL_FOCUS_ID, PYTHON_CONSOLE_FOCUS_ID};
use crate::world::object_definitions::EquipmentSlot;

pub fn spawn_hud(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    hud_minimap_settings: Res<HudMinimapSettings>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
) {
    let theme = theme.clone();
    let palette = *palette;
    commands
        .spawn((
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
                top: px(MENU_BAR_HEIGHT),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Stretch,
                ..default()
            },
            BackgroundColor(Color::NONE),
            HudRoot,
        ))
        .with_children(|parent| {
            parent.spawn((
                Node {
                    flex_grow: 1.0,
                    height: percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ));

            parent
                .spawn((
                    Node {
                        width: px(272.0),
                        height: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::axes(px(10.0), px(10.0)),
                        row_gap: px(10.0),
                        ..default()
                    },
                    RightSidebarRoot,
                    BackgroundColor(palette.surface_sidebar),
                ))
                .with_children(|sidebar| {
                    sidebar
                        .spawn((
                            Node {
                                width: percent(100.0),
                                flex_grow: 1.0,
                                min_height: px(0.0),
                                position_type: PositionType::Relative,
                                ..default()
                            },
                            DockedPanelCanvas,
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|dock_canvas| {
                            spawn_minimap_panel(
                                dock_canvas,
                                DockedPanelState::MINIMAP_PANEL_ID,
                                &asset_server,
                                hud_minimap_settings.zoom,
                                &theme,
                                &palette,
                            );
                            spawn_status_panel(
                                dock_canvas,
                                DockedPanelState::STATUS_PANEL_ID,
                                &theme,
                                &palette,
                            );
                            spawn_equipment_panel(
                                dock_canvas,
                                DockedPanelState::EQUIPMENT_PANEL_ID,
                                &theme,
                                &palette,
                            );
                            spawn_backpack_panel(
                                dock_canvas,
                                DockedPanelState::BACKPACK_PANEL_ID,
                                &theme,
                                &palette,
                            );
                            spawn_docked_panel_canvas(dock_canvas, &theme, &palette);
                        });
                });
        });

    spawn_menu_bar(&mut commands, &theme, &palette);
    spawn_character_sheet_button(&mut commands, &asset_server);
    crate::ui::time_of_day_button::spawn_time_of_day_button(&mut commands, &asset_server);

    // Bottom HUD column: anchored to the screen bottom and stops short of
    // the 272-px right sidebar. Vertical flex stack — quickbar sits on top
    // and the chat/console panel sits beneath. The column is content-sized
    // so hiding the chat lets the quickbar drop flush with the screen edge.
    let bottom_column = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0.0),
                left: px(0.0),
                right: px(272.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(8.0),
                padding: UiRect::all(px(14.0)),
                ..default()
            },
            BottomHudColumn,
            BackgroundColor(Color::NONE),
            HudRoot,
        ))
        .id();

    commands.entity(bottom_column).with_children(|column| {
        crate::ui::quickbar::spawn_quickbar(column, &theme, &palette);
    });

    let chat_area = commands
        .spawn((
            Node {
                width: percent(100.0),
                height: px(260.0),
                flex_direction: FlexDirection::Column,
                position_type: PositionType::Relative,
                ..default()
            },
            ChatAreaContainer,
            BackgroundColor(Color::NONE),
            ChildOf(bottom_column),
        ))
        .id();

    // Chat panel: a read-only terminal widget wrapped in the existing
    // chat surface for label + padding. Fills the full width of the
    // bottom column when visible.
    let chat_panel = commands
        .spawn((
            Node {
                width: percent(100.0),
                height: percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(10.0),
                padding: UiRect::all(px(12.0)),
                ..default()
            },
            ChatPanel,
            BackgroundColor(palette.surface_chat),
            ChildOf(chat_area),
        ))
        .id();
    let theme_for_chat_header = theme.clone();
    commands.entity(chat_panel).with_children(|panel| {
        spawn_chat_header(panel, &theme_for_chat_header, &palette);
    });
    let chat_terminal = spawn_terminal(
        &mut commands,
        TerminalConfig {
            initial_lines: Vec::new(),
            capacity: 256,
            input: Some(TerminalInputConfig {
                prompt: "> ".to_owned(),
                completion: false,
            }),
            focus_id: CHAT_TERMINAL_FOCUS_ID,
            width: percent(100.0),
            height: Val::Auto,
            background: None,
            ..default()
        },
    );
    commands.entity(chat_terminal).insert((
        ChatTerminal,
        Node {
            width: percent(100.0),
            flex_grow: 1.0,
            min_height: px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ));
    commands.entity(chat_panel).add_children(&[chat_terminal]);

    // Python console: read-write terminal hidden until backtick toggles it
    // visible. Same width as the chat panel — replaces it when open.
    let console_panel = commands
        .spawn((
            Node {
                width: percent(100.0),
                height: percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(10.0),
                padding: UiRect::all(px(12.0)),
                display: Display::None,
                position_type: PositionType::Absolute,
                top: px(0.0),
                left: px(0.0),
                ..default()
            },
            PythonConsolePanel,
            BackgroundColor(palette.surface_chat),
            ChildOf(chat_area),
        ))
        .id();
    commands.entity(console_panel).with_children(|panel| {
        spawn_python_console_header(panel, &theme, &palette);
    });
    let console_terminal = spawn_terminal(
        &mut commands,
        TerminalConfig {
            initial_lines: vec![
                TerminalLine::new(
                    "[System] Press ` to toggle the Python console.",
                    LineStyle::System,
                ),
                TerminalLine::new(
                    "[Hint] world.player(), world.objects(), world.spawn(type, x, y), world.give(type, n).",
                    LineStyle::System,
                ),
            ],
            capacity: 512,
            input: Some(TerminalInputConfig {
                prompt: ">>> ".to_owned(),
                completion: true,
            }),
            focus_id: PYTHON_CONSOLE_FOCUS_ID,
            width: percent(100.0),
            height: Val::Auto,
            background: Some(palette.surface_console_output),
            ..default()
        },
    );
    commands.entity(console_terminal).insert((
        PythonConsoleTerminal,
        Node {
            width: percent(100.0),
            flex_grow: 1.0,
            min_height: px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    ));
    commands
        .entity(console_panel)
        .add_children(&[console_terminal]);

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(-300.0),
                top: px(-300.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: px(8.0),
                padding: UiRect::axes(px(8.0), px(4.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            DragPreviewRoot,
            HudRoot,
            Visibility::Hidden,
            GlobalZIndex(i32::MAX - 6),
            ImageNode::new(theme.panel_frame.clone())
                .with_mode(theme.panel_image_mode())
                .with_color(Color::WHITE),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_accent),
        ))
        .with_children(|preview| {
            preview
                .spawn(Node {
                    width: px(32.0),
                    height: px(32.0),
                    align_items: AlignItems::FlexEnd,
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|icon_slot| {
                    icon_slot.spawn((
                        Node {
                            width: px(32.0),
                            height: px(32.0),
                            position_type: PositionType::Absolute,
                            ..default()
                        },
                        ImageNode::default(),
                        DragPreviewImage,
                        Visibility::Hidden,
                    ));
                    icon_slot.spawn((
                        Text::new(""),
                        DragPreviewQuantity,
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(palette.text_quantity),
                        Node {
                            position_type: PositionType::Absolute,
                            bottom: px(0.0),
                            right: px(2.0),
                            ..default()
                        },
                        Visibility::Hidden,
                    ));
                });
            preview.spawn((
                Text::new(""),
                DragPreviewLabel,
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(palette.text_accent),
            ));
        });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(-400.0),
                top: px(-400.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(8.0), px(4.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            ItemTooltipRoot,
            HudRoot,
            Visibility::Hidden,
            GlobalZIndex(i32::MAX - 4),
            ImageNode::new(theme.panel_frame.clone())
                .with_mode(theme.panel_image_mode())
                .with_color(Color::WHITE),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_accent),
        ))
        .with_children(|tooltip| {
            tooltip.spawn((
                Text::new(""),
                ItemTooltipLabel,
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(palette.text_primary),
            ));
        });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: px(140.0),
                left: px(-300.0),
                top: px(-300.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6.0)),
                row_gap: px(4.0),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            ContextMenuRoot,
            HudRoot,
            Visibility::Hidden,
            GlobalZIndex(i32::MAX - 10),
            ImageNode::new(theme.panel_frame.clone())
                .with_mode(theme.panel_image_mode())
                .with_color(Color::WHITE),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_accent),
        ))
        .with_children(|menu| {
            spawn_context_button(menu, &theme, &palette, "Talk", ContextMenuTalkButton);
            spawn_context_button(menu, &theme, &palette, "Trade", ContextMenuTradeButton);
            spawn_context_button(
                menu,
                &theme,
                &palette,
                "Offer to Trade",
                ContextMenuOfferToTradeButton,
            );
            spawn_context_button(menu, &theme, &palette, "Attack", ContextMenuAttackButton);
            spawn_context_button(menu, &theme, &palette, "Use", ContextMenuUseButton);
            spawn_context_button(menu, &theme, &palette, "Use On", ContextMenuUseOnButton);
            spawn_context_button(
                menu,
                &theme,
                &palette,
                "Take...",
                ContextMenuTakePartialButton,
            );
            spawn_context_button(menu, &theme, &palette, "Inspect", ContextMenuInspectButton);
            spawn_context_button(menu, &theme, &palette, "Open", ContextMenuOpenButton);
            spawn_context_button(
                menu,
                &theme,
                &palette,
                "Interact",
                ContextMenuInteractButton,
            );
            spawn_context_button(
                menu,
                &theme,
                &palette,
                "Pick Lock",
                ContextMenuPickLockButton,
            );
            spawn_context_button(
                menu,
                &theme,
                &palette,
                "Force Lock",
                ContextMenuForceLockButton,
            );
            spawn_context_button(menu, &theme, &palette, "Use Key", ContextMenuUseKeyButton);
            spawn_context_button(menu, &theme, &palette, "Hide", ContextMenuHideButton);
            spawn_context_button(
                menu,
                &theme,
                &palette,
                "Read",
                crate::ui::components::ContextMenuReadButton,
            );
        });

    spawn_take_partial_popup(&mut commands, &theme, &palette);
    // Trade and Dialog windows are no longer pre-spawned —
    // `sync_trade_window_lifecycle` (`crate::ui::trade`) and
    // `sync_dialog_window_lifecycle` (`crate::ui::dialog`) spawn them
    // dynamically when a session opens.
}

fn spawn_take_partial_popup(commands: &mut Commands, theme: &UiThemeAssets, palette: &Palette) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                left: px(0.0),
                top: px(0.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            TakePartialPopupRoot,
            HudRoot,
            Visibility::Hidden,
            GlobalZIndex(i32::MAX - 5),
            BackgroundColor(palette.surface_overlay_dim),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    ThemedPanel,
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(10.0),
                        padding: UiRect::all(px(16.0)),
                        border: UiRect::all(px(1.0)),
                        ..default()
                    },
                    ImageNode::new(theme.panel_frame.clone())
                        .with_mode(theme.panel_image_mode())
                        .with_color(Color::WHITE),
                    BackgroundColor(Color::NONE),
                    BorderColor::all(palette.border_accent),
                ))
                .with_children(|dialog| {
                    dialog.spawn((
                        Text::new("How many?"),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(palette.text_primary),
                    ));

                    dialog
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: px(10.0),
                            ..default()
                        },))
                        .with_children(|row| {
                            spawn_small_button(
                                row,
                                theme,
                                palette,
                                ButtonStyle::Secondary,
                                "-",
                                TakePartialDecButton,
                            );
                            row.spawn((
                                Text::new("1"),
                                TakePartialAmountLabel,
                                TextFont {
                                    font_size: 18.0,
                                    ..default()
                                },
                                TextColor(palette.text_quantity),
                                Node {
                                    min_width: px(48.0),
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                            ));
                            spawn_small_button(
                                row,
                                theme,
                                palette,
                                ButtonStyle::Secondary,
                                "+",
                                TakePartialIncButton,
                            );
                        });

                    dialog
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: px(8.0),
                            ..default()
                        },))
                        .with_children(|row| {
                            spawn_small_button(
                                row,
                                theme,
                                palette,
                                ButtonStyle::Primary,
                                "Take",
                                TakePartialConfirmButton,
                            );
                            spawn_small_button(
                                row,
                                theme,
                                palette,
                                ButtonStyle::Secondary,
                                "Cancel",
                                TakePartialCancelButton,
                            );
                        });
                });
        });
}

pub(crate) fn spawn_small_button<T: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    style: ButtonStyle,
    label: &str,
    marker: T,
) {
    let (bg, border, text) = idle_colors(palette, style, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(style),
            marker,
            Node {
                min_width: px(52.0),
                min_height: px(28.0),
                padding: UiRect::axes(px(8.0), px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
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
                Text::new(label),
                TextFont {
                    font_size: 15.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

fn spawn_character_sheet_button(commands: &mut Commands, asset_server: &AssetServer) {
    let portrait: Handle<Image> = asset_server.load("overworld_objects/player/sprite.png");
    commands
        .spawn((
            Button,
            crate::ui::components::CharacterSheetButton,
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                top: px(MENU_BAR_HEIGHT + 12.0),
                right: px(294.0),
                width: px(48.0),
                height: px(48.0),
                padding: UiRect::all(px(4.0)),
                border: UiRect::all(px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.10, 0.08, 0.04, 0.92)),
            BorderColor::all(Color::srgb(0.60, 0.45, 0.24)),
            GlobalZIndex(50),
        ))
        .with_children(|button| {
            button.spawn((
                Node {
                    width: px(36.0),
                    height: px(36.0),
                    ..default()
                },
                ImageNode::new(portrait),
            ));
        });
}

fn spawn_status_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let undock_image = theme.undock_button.clone();
    spawn_docked_panel_with_extras(
        parent,
        panel_id,
        theme,
        palette,
        |title_extras| {
            spawn_themed_icon_button(title_extras, undock_image, StatusPanelUndockButton);
        },
        |body| spawn_status_panel_body(body, palette),
    );
}

/// Body contents of the status panel — HP/MP/XP bars plus the regen,
/// magic-effects, and carry-weight labels. Shared between the docked
/// (`spawn_status_panel`) and floating (`spawn_floating_status_window`)
/// variants so both stay in sync.
pub(crate) fn spawn_status_panel_body(parent: &mut ChildSpawnerCommands, palette: &Palette) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(8.0),
                ..default()
            },
            StatusPanelContent,
            BackgroundColor(Color::NONE),
        ))
        .with_children(|panel| {
            spawn_vital_bar(
                panel,
                palette,
                "HP",
                palette.vital_health_fill,
                HealthFill,
                HealthLabel,
            );
            spawn_vital_bar(
                panel,
                palette,
                "MP",
                palette.vital_mana_fill,
                ManaFill,
                ManaLabel,
            );
            spawn_vital_bar(
                panel,
                palette,
                "XP",
                palette.vital_experience_fill,
                ExperienceFill,
                ExperienceLabel,
            );
            // Regen buff timer label. Always rendered; `sync_regen_buff_label`
            // writes the timer string while the buff is active and clears it
            // back to "" otherwise (empty Text renders as nothing, no need to
            // toggle Visibility — fewer moving parts than a hidden/visible
            // flip during required-component setup).
            panel.spawn((
                Text::new(""),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(palette.text_accent),
                RegenBuffLabel,
            ));
            // Active magical effects (spell-driven buffs). Same shape as the
            // regen-buff label — always rendered, sync system writes the text.
            panel.spawn((
                Text::new(""),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(palette.text_accent),
                MagicEffectsLabel,
            ));
            // Carry weight readout. Always rendered; `sync_carry_weight_label`
            // updates the text every frame the cached client value changes.
            panel.spawn((
                Text::new(""),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(palette.text_value),
                CarryWeightLabel,
            ));
        });
}

fn spawn_equipment_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let undock_image = theme.undock_button.clone();
    let theme_clone = theme.clone();
    spawn_docked_panel_with_extras(
        parent,
        panel_id,
        theme,
        palette,
        |title_extras| {
            spawn_themed_icon_button(title_extras, undock_image, EquipmentPanelUndockButton);
        },
        |body| spawn_equipment_panel_body(body, &theme_clone, palette),
    );
}

/// Body contents of the equipment panel — the paperdoll grid of slots.
/// Shared between the docked (`spawn_equipment_panel`) and floating
/// (`spawn_floating_equipment_window`) variants so both stay in sync.
pub(crate) fn spawn_equipment_panel_body(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                align_items: AlignItems::Center,
                ..default()
            },
            EquipmentPanelContent,
            BackgroundColor(Color::NONE),
        ))
        .with_children(|paperdoll| {
            spawn_slot_row(paperdoll, theme, palette, &["Amulet"]);
            spawn_slot_row(paperdoll, theme, palette, &["Helmet"]);
            spawn_slot_row(paperdoll, theme, palette, &["Weapon", "Armor", "Shield"]);
            spawn_slot_row(paperdoll, theme, palette, &["Legs", "Backpack", "Ring"]);
            spawn_slot_row(paperdoll, theme, palette, &["Boots", "Ammo"]);
        });
}

fn spawn_backpack_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let undock_image = theme.undock_button.clone();
    let theme_clone = theme.clone();
    spawn_docked_panel_with_extras(
        parent,
        panel_id,
        theme,
        palette,
        |title_extras| {
            spawn_themed_icon_button(title_extras, undock_image, BackpackPanelUndockButton);
        },
        |body| spawn_backpack_panel_body(body, &theme_clone, palette),
    );
}

/// Body contents of the backpack panel — the 4x4 inventory grid.
/// Shared between docked (`spawn_backpack_panel`) and floating
/// (`spawn_floating_backpack_window`) variants.
pub(crate) fn spawn_backpack_panel_body(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                ..default()
            },
            BackpackPanelContent,
            BackgroundColor(Color::NONE),
        ))
        .with_children(|grid| {
            for row_index in 0..4 {
                grid.spawn((
                    Node {
                        width: percent(100.0),
                        column_gap: px(6.0),
                        ..default()
                    },
                    BackpackSlotRow { row_index },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|row| {
                    for column in 0..4 {
                        let index = row_index * 4 + column;
                        spawn_backpack_slot(row, theme, palette, index);
                    }
                });
            }
        });
}

fn spawn_docked_panel_canvas(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    spawn_nearby_npcs_panel(parent, theme, palette);

    for offset in 0..DockedPanelState::MAX_OPEN_CONTAINERS {
        spawn_container_panel(
            parent,
            DockedPanelState::FIRST_CONTAINER_PANEL_ID + offset,
            theme,
            palette,
        );
    }
}

pub(crate) fn spawn_trade_column(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    title: &str,
    column: TradeColumn,
) {
    parent
        .spawn((
            Node {
                flex_basis: percent(0.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(2.0),
                min_height: px(0.0),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|col| {
            col.spawn((
                Text::new(title.to_owned()),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(palette.text_value),
            ));
            col.spawn((
                Node {
                    width: percent(100.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2.0),
                    min_height: px(0.0),
                    padding: UiRect::all(px(4.0)),
                    border: UiRect::all(px(1.0)),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                column,
                BackgroundColor(palette.surface_raised),
                BorderColor::all(palette.border_slot),
            ));
        });
}

pub(crate) fn spawn_trade_button<T: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    label_kind: TradeButtonLabel,
    marker: T,
    style: ButtonStyle,
) {
    let (bg, border, text) = idle_colors(palette, style, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(style),
            marker,
            Node {
                flex_grow: 1.0,
                min_height: px(28.0),
                border: UiRect::all(px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
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
                label_kind,
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

fn spawn_nearby_npcs_panel(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let undock_image = theme.undock_button.clone();
    spawn_docked_panel_with_extras(
        parent,
        DockedPanelState::NEARBY_NPCS_PANEL_ID,
        theme,
        palette,
        |title_extras| {
            spawn_themed_icon_button(title_extras, undock_image, NearbyNpcsPanelUndockButton);
        },
        |body| spawn_nearby_npcs_panel_body(body, palette),
    );
}

/// Body of the Nearby NPCs panel — a scrollable column whose rows are
/// (re)spawned by `sync_nearby_npcs_panel` whenever the visible NPC set or
/// its sort order changes. Shared between docked and floating variants via
/// the `MountablePanel` impl.
pub(crate) fn spawn_nearby_npcs_panel_body(parent: &mut ChildSpawnerCommands, palette: &Palette) {
    parent.spawn((
        Node {
            width: percent(100.0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(2.0),
            min_height: px(0.0),
            padding: UiRect::all(px(4.0)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        NearbyNpcsList,
        ScrollPosition::default(),
        BackgroundColor(palette.surface_raised),
    ));
}

fn spawn_container_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let undock_image = theme.undock_button.clone();
    spawn_docked_panel_with_extras(
        parent,
        panel_id,
        theme,
        palette,
        |title_extras| {
            spawn_themed_icon_button(
                title_extras,
                undock_image,
                ContainerPanelUndockButton { panel_id },
            );
        },
        |body| spawn_container_panel_body(body, theme, palette, panel_id),
    );
}

/// Body builder for the 4×4 container slot grid. Shared between the
/// pre-spawned docked panel pool and the floating window spawned by
/// `sync_container_floating_lifecycle` — both call into the same slot
/// resolution path keyed on `panel_id`, so the floating window's items
/// follow the docked panel's `object_id` automatically.
pub(crate) fn spawn_container_panel_body(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    panel_id: usize,
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                ..default()
            },
            ContainerPanelContent,
            BackgroundColor(Color::NONE),
        ))
        .with_children(|grid| {
            for row_index in 0..4 {
                grid.spawn((
                    Node {
                        width: percent(100.0),
                        column_gap: px(6.0),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|row| {
                    for column in 0..4 {
                        let slot_index = row_index * 4 + column;
                        spawn_open_container_slot(row, theme, palette, panel_id, slot_index);
                    }
                });
            }
        });
}

/// Spawn a docked sidebar panel — wood-grain framed container with a
/// draggable title bar, close-X, body area, and resize handle. The
/// `spawn_title_extras` closure runs in the title-bar row right before
/// the close-X (used by panels that want additional title-bar buttons
/// like an undock arrow); pass `|_| {}` for no extras.
fn spawn_docked_panel_with_extras(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
    spawn_title_extras: impl FnOnce(&mut ChildSpawnerCommands),
    spawn_body: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                height: px(DockedPanelState::DEFAULT_CONTAINER_PANEL_HEIGHT),
                min_height: px(0.0),
                position_type: PositionType::Absolute,
                left: px(0.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            DockedPanelRoot { panel_id },
            ThemedPanel,
            Visibility::Hidden,
            ImageNode::new(theme.panel_frame.clone())
                .with_mode(theme.panel_image_mode())
                .with_color(Color::WHITE),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_slot),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: percent(100.0),
                        min_height: px(30.0),
                        padding: UiRect::axes(px(8.0), px(6.0)),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        column_gap: px(6.0),
                        ..default()
                    },
                    DockedPanelDragHandle { panel_id },
                    ImageNode::new(theme.title_bar.clone())
                        .with_mode(theme.title_bar_image_mode())
                        .with_color(Color::WHITE),
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|title_row| {
                    title_row.spawn((
                        Text::new(""),
                        DockedPanelTitle { panel_id },
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(palette.text_primary),
                    ));

                    // Right-cluster: any caller-supplied title-bar
                    // buttons (e.g. the status panel's undock arrow)
                    // sit directly next to the close-X. Wrapping them
                    // in a single row keeps SpaceBetween from pushing
                    // a lone extras button to the centre.
                    title_row
                        .spawn((
                            Node {
                                column_gap: px(4.0),
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|button_cluster| {
                            spawn_title_extras(button_cluster);
                            spawn_themed_close_button(
                                button_cluster,
                                theme,
                                DockedPanelCloseButton { panel_id },
                            );
                        });
                });

            panel
                .spawn((
                    Node {
                        width: percent(100.0),
                        flex_grow: 1.0,
                        min_height: px(0.0),
                        padding: UiRect::all(px(8.0)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                    DockedPanelBody { panel_id },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(spawn_body);

            panel.spawn((
                Node {
                    width: percent(100.0),
                    height: px(10.0),
                    ..default()
                },
                DockedPanelResizeHandle { panel_id },
                ImageNode::new(theme.resize_grip.clone())
                    .with_mode(theme.resize_grip_image_mode())
                    .with_color(Color::WHITE),
                BackgroundColor(Color::NONE),
            ));
        });
}

/// Python-console panel header row: label on the left, a small "Restart"
/// button on the right. The button is `PythonConsoleRestartButton`-tagged
/// so `scripting::systems::handle_python_console_restart_button` can pick
/// up the click and rebuild the embedded interpreter scope.
fn spawn_python_console_header(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new("Python Console"),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(palette.text_primary),
            ));
            spawn_themed_button(
                row,
                theme,
                palette,
                ButtonStyle::Secondary,
                Node {
                    padding: UiRect::axes(px(10.0), px(3.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                "Restart",
                12.0,
                PythonConsoleRestartButton,
            );
        });
}

/// Chat-panel header row: label on the left, close-X on the right. The
/// close button is `BottomPanelHideButton`-tagged so the visibility-toggle
/// system can pick up the click.
fn spawn_chat_header(parent: &mut ChildSpawnerCommands, theme: &UiThemeAssets, palette: &Palette) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new("Chat"),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(palette.text_primary),
            ));
            spawn_themed_close_button(row, theme, BottomPanelHideButton);
        });
}

fn spawn_vital_bar<T: Bundle>(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    label: &str,
    fill_color: Color,
    marker: T,
    value_marker: impl Component,
) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                min_height: px(18.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6.0),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|bar_group| {
            bar_group.spawn((
                Text::new(format!("{label}:")),
                value_marker,
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(palette.text_muted),
                Node {
                    width: px(28.0),
                    ..default()
                },
            ));

            spawn_retro_bar(
                bar_group,
                palette,
                RetroBarStyle::default()
                    .with_fill(fill_color)
                    .with_height(14.0),
                marker,
            );
        });
}

fn spawn_slot_row(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    labels: &[&str],
) {
    parent
        .spawn((
            Node {
                column_gap: px(6.0),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|row| {
            for label in labels {
                spawn_equipment_slot(row, theme, palette, Some(label));
            }
        });
}

fn spawn_equipment_slot(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: Option<&str>,
) {
    let slot = EquipmentSlot::ALL
        .into_iter()
        .find(|slot| Some(slot.label()) == label)
        .expect("Unknown equipment slot label");
    let (bg, border, _) = idle_colors(palette, ButtonStyle::Slot, false);

    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Slot),
            EquipmentSlotButton,
            ItemSlotButton {
                kind: ItemSlotKind::Equipment(slot),
            },
            Node {
                width: px(38.0),
                height: px(38.0),
                border: UiRect::all(px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            ImageNode::new(theme.slot_frame.clone())
                .with_mode(theme.slot_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|slot_node| {
            slot_node.spawn((
                Node {
                    width: px(32.0),
                    height: px(32.0),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                ImageNode::default(),
                ItemSlotImage {
                    kind: ItemSlotKind::Equipment(slot),
                },
                EquipmentSlotImage,
                Visibility::Hidden,
            ));

            if let Some(label) = label {
                slot_node.spawn((
                    Text::new(label),
                    TextFont {
                        font_size: 8.0,
                        ..default()
                    },
                    TextColor(palette.text_label_slot),
                    EquipmentSlotLabel { slot },
                ));
            }
        });
}

fn spawn_backpack_slot(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    index: usize,
) {
    let (bg, border, _) = idle_colors(palette, ButtonStyle::Slot, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Slot),
            ContainerSlotButton,
            ItemSlotButton {
                kind: ItemSlotKind::Backpack(index),
            },
            Node {
                width: px(38.0),
                height: px(38.0),
                border: UiRect::all(px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                position_type: PositionType::Relative,
                ..default()
            },
            ImageNode::new(theme.slot_frame.clone())
                .with_mode(theme.slot_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|slot| {
            slot.spawn((
                Node {
                    width: px(24.0),
                    height: px(24.0),
                    ..default()
                },
                ImageNode::default(),
                ItemSlotImage {
                    kind: ItemSlotKind::Backpack(index),
                },
                ContainerSlotImage,
                Visibility::Hidden,
            ));
            slot.spawn((
                Text::new(""),
                TextFont {
                    font_size: 9.0,
                    ..default()
                },
                TextColor(palette.text_quantity),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: px(1.0),
                    right: px(2.0),
                    ..default()
                },
                ItemSlotQuantityLabel {
                    kind: ItemSlotKind::Backpack(index),
                },
                Visibility::Hidden,
            ));
        });
}

fn spawn_open_container_slot(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    panel_id: usize,
    slot_index: usize,
) {
    let kind = ItemSlotKind::OpenContainer {
        panel_id,
        slot_index,
    };
    let (bg, border, _) = idle_colors(palette, ButtonStyle::Slot, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Slot),
            ContainerSlotButton,
            ItemSlotButton { kind },
            Node {
                width: px(42.0),
                height: px(42.0),
                border: UiRect::all(px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                position_type: PositionType::Relative,
                ..default()
            },
            ImageNode::new(theme.slot_frame.clone())
                .with_mode(theme.slot_image_mode())
                .with_color(bg),
            BackgroundColor(Color::NONE),
            BorderColor::all(border),
        ))
        .with_children(|slot| {
            slot.spawn((
                Node {
                    width: px(28.0),
                    height: px(28.0),
                    ..default()
                },
                ImageNode::default(),
                ItemSlotImage { kind },
                ContainerSlotImage,
                Visibility::Hidden,
            ));
            slot.spawn((
                Text::new(""),
                TextFont {
                    font_size: 9.0,
                    ..default()
                },
                TextColor(palette.text_quantity),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: px(1.0),
                    right: px(2.0),
                    ..default()
                },
                ItemSlotQuantityLabel { kind },
                Visibility::Hidden,
            ));
        });
}

fn spawn_context_button<T: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    marker: T,
) {
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Secondary, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Secondary),
            marker,
            Node {
                width: percent(100.0),
                min_height: px(28.0),
                padding: UiRect::axes(px(8.0), px(4.0)),
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
                Text::new(label),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

fn spawn_minimap_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    asset_server: &AssetServer,
    zoom: crate::ui::resources::MinimapZoom,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let undock_image = theme.undock_button.clone();
    let theme_clone = theme.clone();
    let palette_clone = *palette;
    let asset_server_clone = asset_server.clone();
    spawn_docked_panel_with_extras(
        parent,
        panel_id,
        theme,
        palette,
        |title_extras| {
            spawn_themed_icon_button(title_extras, undock_image, MinimapPanelUndockButton);
        },
        move |body| {
            spawn_minimap_panel_body_for_mode(
                body,
                &theme_clone,
                &palette_clone,
                &asset_server_clone,
                BodyMode::Docked,
                zoom,
            );
        },
    );
}

/// Which presentation context the minimap body is being built for. The
/// docked variant renders the small HUD-sized image with HUD zoom
/// markers; the floating variant renders the larger image used by the
/// pop-out window, with separate floating zoom markers/state.
#[derive(Clone, Copy)]
pub(crate) enum BodyMode {
    Docked,
    Floating,
}

/// Body contents of the minimap panel — rendered minimap image plus
/// zoom +/− controls. The docked spawn path calls this with
/// `BodyMode::Docked`; `MountablePanel::spawn_body` (only invoked for
/// floating spawns) calls it with `BodyMode::Floating`.
pub(crate) fn spawn_minimap_panel_body_for_mode(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    asset_server: &AssetServer,
    body_mode: BodyMode,
    zoom: crate::ui::resources::MinimapZoom,
) {
    let image_handle = asset_server.add(make_minimap_image(zoom));
    let (image_size, minimap_mode) = match body_mode {
        BodyMode::Docked => (HUD_MINIMAP_SIZE, MinimapMode::HudSmall),
        BodyMode::Floating => (FULL_MAP_BODY_SIZE, MinimapMode::FullscreenLarge),
    };
    parent
        .spawn((Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6.0),
            ..default()
        },))
        .with_children(|container| {
            container.spawn((
                Node {
                    width: px(image_size),
                    height: px(image_size),
                    position_type: PositionType::Relative,
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(palette.surface_minimap_bg),
                ImageNode::new(image_handle.clone()).with_mode(NodeImageMode::Stretch),
                MinimapView { mode: minimap_mode },
                MinimapCanvas {
                    image_handle: image_handle.clone(),
                    last_zoom: Some(zoom),
                },
            ));

            container
                .spawn((Node {
                    width: percent(100.0),
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    column_gap: px(6.0),
                    ..default()
                },))
                .with_children(|row| match body_mode {
                    BodyMode::Docked => {
                        spawn_zoom_button(row, theme, palette, "-", HudMinimapZoomOutButton);
                        row.spawn((
                            Text::new(zoom.label()),
                            HudMinimapZoomLabel,
                            TextFont {
                                font_size: 14.0,
                                ..default()
                            },
                            TextColor(palette.text_primary),
                            Node {
                                min_width: px(64.0),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                        ));
                        spawn_zoom_button(row, theme, palette, "+", HudMinimapZoomInButton);
                    }
                    BodyMode::Floating => {
                        spawn_zoom_button(row, theme, palette, "-", FloatingMinimapZoomOutButton);
                        row.spawn((
                            Text::new(zoom.label()),
                            FloatingMinimapZoomLabel,
                            TextFont {
                                font_size: 14.0,
                                ..default()
                            },
                            TextColor(palette.text_primary),
                            Node {
                                min_width: px(64.0),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                        ));
                        spawn_zoom_button(row, theme, palette, "+", FloatingMinimapZoomInButton);
                    }
                });
        });
}

fn spawn_zoom_button<T: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    marker: T,
) {
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Secondary, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Secondary),
            marker,
            Node {
                width: px(26.0),
                height: px(22.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
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
                Text::new(label),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}
