use bevy::prelude::*;
use bevy::text::{Justify, LineBreak, TextLayout};
use bevy::ui::widget::NodeImageMode;

use crate::ui::components::{
    BackpackPanelContent, BackpackSlotRow, ChatLogText, ContainerPanelContent, ContainerSlotButton,
    ContainerSlotImage, ContextMenuAttackButton, ContextMenuInspectButton,
    ContextMenuInteractButton, ContextMenuOpenButton, ContextMenuRoot,
    ContextMenuTakePartialButton, ContextMenuTalkButton, ContextMenuUseButton,
    ContextMenuUseOnButton, CurrentCombatTargetLabel, CurrentTargetPanelContent,
    DialogPanelBodyText, DialogPanelCloseButton, DialogPanelContinueButton,
    DialogPanelOptionsContainer, DialogPanelRoot, DialogPanelSpeakerLabel, DockedPanelBody,
    DockedPanelCanvas, DockedPanelCloseButton, DockedPanelDragHandle, DockedPanelResizeHandle,
    DockedPanelRoot, DockedPanelTitle, DragPreviewLabel, DragPreviewRoot, EquipmentPanelContent,
    EquipmentSlotButton, EquipmentSlotImage, FullMapBodyRoot, FullMapCloseButton,
    FullMapWindowRoot, FullMapZoomInButton, FullMapZoomLabel, FullMapZoomOutButton, HealthFill,
    HealthLabel, HudMinimapZoomInButton, HudMinimapZoomLabel, HudMinimapZoomOutButton,
    ItemSlotButton, ItemSlotImage, ItemSlotKind, ItemSlotQuantityLabel, ManaFill, ManaLabel,
    MinimapCanvas, MinimapMode, MinimapView, PythonConsoleInput, PythonConsoleOutput,
    PythonConsoleOutputViewport, PythonConsolePanel, PythonConsoleScrollbarThumb, RightSidebarRoot,
    StatusPanelContent, TakePartialAmountLabel, TakePartialCancelButton, TakePartialConfirmButton,
    TakePartialDecButton, TakePartialIncButton, TakePartialPopupRoot,
};
use crate::ui::menu_bar::{spawn_menu_bar, MENU_BAR_HEIGHT};
use crate::ui::minimap::{make_minimap_image, FULL_MAP_BODY_SIZE, HUD_MINIMAP_SIZE};
use crate::ui::resources::{DockedPanelState, FullMapWindowState, HudMinimapSettings};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton, ThemedPanel};
use crate::ui::theme::{Palette, UiThemeAssets};
use crate::world::object_definitions::EquipmentSlot;

pub fn spawn_hud(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    hud_minimap_settings: Res<HudMinimapSettings>,
    full_map_state: Res<FullMapWindowState>,
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
                                &mut images,
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

    spawn_full_map_window(
        &mut commands,
        &mut images,
        full_map_state.zoom,
        &theme,
        &palette,
    );
    spawn_menu_bar(&mut commands, &theme, &palette);

    commands
        .spawn((
            Node {
                width: percent(100.0),
                height: px(360.0),
                position_type: PositionType::Absolute,
                bottom: px(0.0),
                left: px(0.0),
                padding: UiRect::all(px(14.0)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Node {
                        width: percent(30.0),
                        height: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10.0),
                        padding: UiRect::all(px(12.0)),
                        ..default()
                    },
                    BackgroundColor(palette.surface_chat),
                ))
                .with_children(|chat_panel| {
                    spawn_panel_label(chat_panel, "Chat", &palette);
                    chat_panel.spawn((
                        Text::new(""),
                        ChatLogText,
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(palette.text_muted),
                        TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                        Node {
                            width: percent(100.0),
                            ..default()
                        },
                    ));
                });

            parent
                .spawn((
                    Node {
                        width: percent(58.0),
                        height: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10.0),
                        padding: UiRect::all(px(12.0)),
                        ..default()
                    },
                    PythonConsolePanel,
                    BackgroundColor(palette.surface_chat),
                ))
                .with_children(|chat_panel| {
                    spawn_panel_label(chat_panel, "Python Console", &palette);

                    chat_panel
                        .spawn((
                            Node {
                                width: percent(100.0),
                                flex_grow: 1.0,
                                min_height: px(0.0),
                                column_gap: px(8.0),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|output_row| {
                            output_row
                                .spawn((
                                    Node {
                                        flex_grow: 1.0,
                                        min_height: px(0.0),
                                        overflow: Overflow::clip(),
                                        padding: UiRect {
                                            left: px(8.0),
                                            right: px(8.0),
                                            top: px(8.0),
                                            bottom: px(14.0),
                                        },
                                        ..default()
                                    },
                                    PythonConsoleOutputViewport,
                                    BackgroundColor(palette.surface_console_output),
                                ))
                                .with_children(|output_viewport| {
                                    output_viewport.spawn((
                                        Text::new(""),
                                        PythonConsoleOutput,
                                        TextFont {
                                            font_size: 16.0,
                                            ..default()
                                        },
                                        TextColor(palette.text_muted),
                                        TextLayout::new(Justify::Left, LineBreak::WordOrCharacter),
                                        Node {
                                            width: percent(100.0),
                                            ..default()
                                        },
                                    ));
                                });

                            output_row
                                .spawn((
                                    Node {
                                        width: px(10.0),
                                        height: percent(100.0),
                                        padding: UiRect::vertical(px(2.0)),
                                        ..default()
                                    },
                                    BackgroundColor(palette.surface_scrollbar_track),
                                ))
                                .with_children(|track| {
                                    track
                                        .spawn((
                                            Node {
                                                width: percent(100.0),
                                                height: percent(100.0),
                                                position_type: PositionType::Relative,
                                                ..default()
                                            },
                                            BackgroundColor(Color::NONE),
                                        ))
                                        .with_children(|thumb_parent| {
                                            thumb_parent.spawn((
                                                Node {
                                                    width: percent(100.0),
                                                    height: percent(100.0),
                                                    min_height: px(20.0),
                                                    position_type: PositionType::Absolute,
                                                    top: px(0.0),
                                                    ..default()
                                                },
                                                PythonConsoleScrollbarThumb,
                                                BackgroundColor(palette.surface_scrollbar_thumb),
                                            ));
                                        });
                                });
                        });

                    chat_panel.spawn((
                        Node {
                            width: percent(100.0),
                            min_height: px(34.0),
                            padding: UiRect::axes(px(6.0), px(4.0)),
                            ..default()
                        },
                        BackgroundColor(palette.surface_console_input),
                        Text::new(">>> "),
                        PythonConsoleInput,
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(palette.text_accent),
                    ));
                });
        });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: px(92.0),
                height: px(32.0),
                left: px(-200.0),
                top: px(-200.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(8.0), px(4.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            DragPreviewRoot,
            Visibility::Hidden,
            ImageNode::new(theme.panel_frame.clone())
                .with_mode(theme.panel_image_mode())
                .with_color(palette.surface_panel),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_accent),
        ))
        .with_children(|preview| {
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
            Visibility::Hidden,
            GlobalZIndex(i32::MAX - 10),
            ImageNode::new(theme.panel_frame.clone())
                .with_mode(theme.panel_image_mode())
                .with_color(palette.surface_panel),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_accent),
        ))
        .with_children(|menu| {
            spawn_context_button(menu, &theme, &palette, "Talk", ContextMenuTalkButton);
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
        });

    spawn_take_partial_popup(&mut commands, &theme, &palette);
    spawn_dialog_panel(&mut commands, &theme, &palette);
}

fn spawn_dialog_panel(commands: &mut Commands, theme: &UiThemeAssets, palette: &Palette) {
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
            DialogPanelRoot,
            Visibility::Hidden,
            GlobalZIndex(i32::MAX - 8),
            BackgroundColor(palette.surface_overlay_dim),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    ThemedPanel,
                    Node {
                        width: px(480.0),
                        max_width: percent(90.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(12.0),
                        padding: UiRect::all(px(18.0)),
                        border: UiRect::all(px(1.0)),
                        ..default()
                    },
                    ImageNode::new(theme.panel_frame.clone())
                        .with_mode(theme.panel_image_mode())
                        .with_color(palette.surface_panel),
                    BackgroundColor(Color::NONE),
                    BorderColor::all(palette.border_accent),
                ))
                .with_children(|panel| {
                    panel
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            width: percent(100.0),
                            ..default()
                        },))
                        .with_children(|header| {
                            header.spawn((
                                Text::new(""),
                                DialogPanelSpeakerLabel,
                                TextFont {
                                    font_size: 16.0,
                                    ..default()
                                },
                                TextColor(palette.text_accent),
                            ));
                            spawn_small_button(
                                header,
                                theme,
                                palette,
                                ButtonStyle::Secondary,
                                "X",
                                DialogPanelCloseButton,
                            );
                        });

                    panel.spawn((
                        Text::new(""),
                        DialogPanelBodyText,
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(palette.text_primary),
                        TextLayout {
                            linebreak: LineBreak::WordBoundary,
                            justify: Justify::Left,
                        },
                        Node {
                            width: percent(100.0),
                            ..default()
                        },
                    ));

                    panel.spawn((
                        Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: px(6.0),
                            width: percent(100.0),
                            ..default()
                        },
                        DialogPanelOptionsContainer,
                    ));

                    spawn_small_button(
                        panel,
                        theme,
                        palette,
                        ButtonStyle::Primary,
                        "Continue",
                        DialogPanelContinueButton,
                    );
                });
        });
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
                        .with_color(palette.surface_panel),
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

fn spawn_small_button<T: Component>(
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

fn spawn_status_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    spawn_docked_panel(parent, panel_id, theme, palette, |body| {
        body.spawn((
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
        });
    });
}

fn spawn_equipment_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    spawn_docked_panel(parent, panel_id, theme, palette, |body| {
        body.spawn((
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
    });
}

fn spawn_backpack_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    spawn_docked_panel(parent, panel_id, theme, palette, |body| {
        body.spawn((
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
    });
}

fn spawn_docked_panel_canvas(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    spawn_current_target_panel(parent, theme, palette);

    for offset in 0..DockedPanelState::MAX_OPEN_CONTAINERS {
        spawn_container_panel(
            parent,
            DockedPanelState::FIRST_CONTAINER_PANEL_ID + offset,
            theme,
            palette,
        );
    }
}

fn spawn_current_target_panel(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    spawn_docked_panel(
        parent,
        DockedPanelState::CURRENT_TARGET_PANEL_ID,
        theme,
        palette,
        |body| {
            body.spawn((
                Text::new("Target: none"),
                CurrentCombatTargetLabel,
                CurrentTargetPanelContent,
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
        },
    );
}

fn spawn_container_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    spawn_docked_panel(parent, panel_id, theme, palette, |body| {
        body.spawn((
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
    });
}

fn spawn_docked_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    theme: &UiThemeAssets,
    palette: &Palette,
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
                .with_color(palette.surface_panel),
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
                        .with_color(palette.surface_title_bar),
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

                    spawn_close_button(
                        title_row,
                        theme,
                        palette,
                        DockedPanelCloseButton { panel_id },
                    );
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
                    height: px(8.0),
                    ..default()
                },
                DockedPanelResizeHandle { panel_id },
                BackgroundColor(palette.surface_resize_handle),
            ));
        });
}

fn spawn_close_button<T: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    marker: T,
) {
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Danger, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Danger),
            marker,
            Node {
                width: px(22.0),
                height: px(22.0),
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
                Text::new("x"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

fn spawn_panel_label(parent: &mut ChildSpawnerCommands, label: &str, palette: &Palette) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(palette.text_primary),
    ));
}

fn spawn_vital_bar<T: Component>(
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

            bar_group
                .spawn((
                    Node {
                        flex_grow: 1.0,
                        height: px(16.0),
                        padding: UiRect::all(px(2.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(palette.surface_vital_bg),
                ))
                .with_children(|bar_container| {
                    bar_container.spawn((
                        Node {
                            width: percent(100.0),
                            height: percent(100.0),
                            ..default()
                        },
                        marker,
                        BackgroundColor(fill_color),
                    ));
                });
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
                    width: px(24.0),
                    height: px(24.0),
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
    images: &mut Assets<Image>,
    zoom: crate::ui::resources::MinimapZoom,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let image_handle = images.add(make_minimap_image(zoom));
    spawn_docked_panel(parent, panel_id, theme, palette, move |body| {
        body.spawn((Node {
            width: percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(6.0),
            ..default()
        },))
            .with_children(|container| {
                container.spawn((
                    Node {
                        width: px(HUD_MINIMAP_SIZE),
                        height: px(HUD_MINIMAP_SIZE),
                        position_type: PositionType::Relative,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(palette.surface_minimap_bg),
                    ImageNode::new(image_handle.clone()).with_mode(NodeImageMode::Stretch),
                    MinimapView {
                        mode: MinimapMode::HudSmall,
                    },
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
                    .with_children(|row| {
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
                    });
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

fn spawn_full_map_window(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    zoom: crate::ui::resources::MinimapZoom,
    theme: &UiThemeAssets,
    palette: &Palette,
) {
    let image_handle = images.add(make_minimap_image(zoom));
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
                display: Display::None,
                ..default()
            },
            FullMapWindowRoot,
            GlobalZIndex(i32::MAX - 8),
            BackgroundColor(palette.surface_overlay_dim),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    ThemedPanel,
                    Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(10.0)),
                        row_gap: px(8.0),
                        border: UiRect::all(px(1.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    ImageNode::new(theme.panel_frame.clone())
                        .with_mode(theme.panel_image_mode())
                        .with_color(palette.surface_panel),
                    BackgroundColor(Color::NONE),
                    BorderColor::all(palette.border_accent),
                ))
                .with_children(|window| {
                    window
                        .spawn((Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceBetween,
                            column_gap: px(12.0),
                            width: px(FULL_MAP_BODY_SIZE),
                            ..default()
                        },))
                        .with_children(|title_row| {
                            title_row.spawn((
                                Text::new("Full Map"),
                                TextFont {
                                    font_size: 18.0,
                                    ..default()
                                },
                                TextColor(palette.text_primary),
                            ));

                            title_row
                                .spawn((Node {
                                    flex_direction: FlexDirection::Row,
                                    align_items: AlignItems::Center,
                                    column_gap: px(6.0),
                                    ..default()
                                },))
                                .with_children(|controls| {
                                    spawn_zoom_button(
                                        controls,
                                        theme,
                                        palette,
                                        "-",
                                        FullMapZoomOutButton,
                                    );
                                    controls.spawn((
                                        Text::new(zoom.label()),
                                        FullMapZoomLabel,
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
                                    spawn_zoom_button(
                                        controls,
                                        theme,
                                        palette,
                                        "+",
                                        FullMapZoomInButton,
                                    );
                                    spawn_close_button(
                                        controls,
                                        theme,
                                        palette,
                                        FullMapCloseButton,
                                    );
                                });
                        });

                    window.spawn((
                        Node {
                            width: px(FULL_MAP_BODY_SIZE),
                            height: px(FULL_MAP_BODY_SIZE),
                            position_type: PositionType::Relative,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(palette.surface_minimap_bg),
                        ImageNode::new(image_handle.clone()).with_mode(NodeImageMode::Stretch),
                        FullMapBodyRoot,
                        MinimapView {
                            mode: MinimapMode::FullscreenLarge,
                        },
                        MinimapCanvas {
                            image_handle: image_handle.clone(),
                            last_zoom: Some(zoom),
                        },
                    ));
                });
        });
}
