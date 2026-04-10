use bevy::prelude::*;
use bevy::text::{Justify, LineBreak, TextLayout};

use crate::ui::components::{
    BackpackPanelContent, BackpackSlotRow, ChatLogText, ContainerPanelContent, ContainerSlotButton,
    ContainerSlotImage, ContextMenuAttackButton, ContextMenuInspectButton, ContextMenuOpenButton,
    ContextMenuRoot, ContextMenuUseButton, ContextMenuUseOnButton, CurrentCombatTargetLabel,
    CurrentTargetPanelContent, DockedPanelBody, DockedPanelCanvas, DockedPanelCloseButton,
    DockedPanelDragHandle, DockedPanelResizeHandle, DockedPanelRoot, DockedPanelTitle,
    DragPreviewLabel, DragPreviewRoot, EquipmentPanelContent, EquipmentSlotButton,
    EquipmentSlotImage, HealthFill, HealthLabel, ItemSlotButton, ItemSlotImage, ItemSlotKind,
    ManaFill, ManaLabel, PythonConsoleInput, PythonConsoleOutput, PythonConsoleOutputViewport,
    PythonConsolePanel, PythonConsoleScrollbarThumb, RightSidebarRoot, StatusPanelContent,
};
use crate::ui::resources::DockedPanelState;
use crate::world::object_definitions::EquipmentSlot;

pub fn spawn_hud(mut commands: Commands) {
    commands
        .spawn((
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
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
                    BackgroundColor(Color::srgba(0.06, 0.06, 0.08, 0.88)),
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
                            spawn_status_panel(dock_canvas, DockedPanelState::STATUS_PANEL_ID);
                            spawn_equipment_panel(
                                dock_canvas,
                                DockedPanelState::EQUIPMENT_PANEL_ID,
                            );
                            spawn_backpack_panel(dock_canvas, DockedPanelState::BACKPACK_PANEL_ID);
                            spawn_docked_panel_canvas(dock_canvas);
                        });
                });
        });

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
                    BackgroundColor(Color::srgba(0.07, 0.08, 0.10, 0.90)),
                ))
                .with_children(|chat_panel| {
                    spawn_panel_label(chat_panel, "Chat");
                    chat_panel.spawn((
                        Text::new(""),
                        ChatLogText,
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.82, 0.83, 0.85)),
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
                    BackgroundColor(Color::srgba(0.07, 0.08, 0.10, 0.90)),
                ))
                .with_children(|chat_panel| {
                    spawn_panel_label(chat_panel, "Python Console");

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
                                    BackgroundColor(Color::srgba(0.04, 0.05, 0.07, 0.92)),
                                ))
                                .with_children(|output_viewport| {
                                    output_viewport.spawn((
                                        Text::new(""),
                                        PythonConsoleOutput,
                                        TextFont {
                                            font_size: 16.0,
                                            ..default()
                                        },
                                        TextColor(Color::srgb(0.82, 0.83, 0.85)),
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
                                    BackgroundColor(Color::srgba(0.10, 0.10, 0.11, 0.95)),
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
                                                BackgroundColor(Color::srgb(0.66, 0.60, 0.38)),
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
                        BackgroundColor(Color::srgba(0.11, 0.10, 0.09, 0.96)),
                        Text::new(">>> "),
                        PythonConsoleInput,
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.92, 0.72)),
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
                ..default()
            },
            DragPreviewRoot,
            Visibility::Hidden,
            BackgroundColor(Color::srgba(0.09, 0.09, 0.10, 0.92)),
            BorderColor::all(Color::srgb(0.60, 0.52, 0.22)),
        ))
        .with_children(|preview| {
            preview.spawn((
                Text::new(""),
                DragPreviewLabel,
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::srgb(0.96, 0.92, 0.72)),
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
                ..default()
            },
            ContextMenuRoot,
            Visibility::Hidden,
            GlobalZIndex(i32::MAX - 10),
            BackgroundColor(Color::srgba(0.09, 0.08, 0.07, 0.97)),
            BorderColor::all(Color::srgb(0.52, 0.44, 0.22)),
        ))
        .with_children(|menu| {
            spawn_context_button(menu, "Attack", ContextMenuAttackButton);
            spawn_context_button(menu, "Use", ContextMenuUseButton);
            spawn_context_button(menu, "Use On", ContextMenuUseOnButton);
            spawn_context_button(menu, "Inspect", ContextMenuInspectButton);
            spawn_context_button(menu, "Open", ContextMenuOpenButton);
        });
}

fn spawn_status_panel(parent: &mut ChildSpawnerCommands, panel_id: usize) {
    spawn_docked_panel(parent, panel_id, |body| {
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
                "HP",
                Color::srgb(0.70, 0.16, 0.18),
                HealthFill,
                HealthLabel,
            );
            spawn_vital_bar(
                panel,
                "MP",
                Color::srgb(0.14, 0.35, 0.78),
                ManaFill,
                ManaLabel,
            );
        });
    });
}

fn spawn_equipment_panel(parent: &mut ChildSpawnerCommands, panel_id: usize) {
    spawn_docked_panel(parent, panel_id, |body| {
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
            spawn_slot_row(paperdoll, &["Amulet"]);
            spawn_slot_row(paperdoll, &["Helmet"]);
            spawn_slot_row(paperdoll, &["Weapon", "Armor", "Shield"]);
            spawn_slot_row(paperdoll, &["Legs", "Backpack", "Ring"]);
            spawn_slot_row(paperdoll, &["Boots"]);
        });
    });
}

fn spawn_backpack_panel(parent: &mut ChildSpawnerCommands, panel_id: usize) {
    spawn_docked_panel(parent, panel_id, |body| {
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
                        spawn_backpack_slot(row, index);
                    }
                });
            }
        });
    });
}

fn spawn_docked_panel_canvas(parent: &mut ChildSpawnerCommands) {
    spawn_current_target_panel(parent);

    for offset in 0..DockedPanelState::MAX_OPEN_CONTAINERS {
        spawn_container_panel(parent, DockedPanelState::FIRST_CONTAINER_PANEL_ID + offset);
    }
}

fn spawn_current_target_panel(parent: &mut ChildSpawnerCommands) {
    spawn_docked_panel(parent, DockedPanelState::CURRENT_TARGET_PANEL_ID, |body| {
        body.spawn((
            Text::new("Target: none"),
            CurrentCombatTargetLabel,
            CurrentTargetPanelContent,
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(Color::srgb(0.86, 0.84, 0.78)),
            Node {
                width: percent(100.0),
                ..default()
            },
        ));
    });
}

fn spawn_container_panel(parent: &mut ChildSpawnerCommands, panel_id: usize) {
    spawn_docked_panel(parent, panel_id, |body| {
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
                        spawn_open_container_slot(row, panel_id, slot_index);
                    }
                });
            }
        });
    });
}

fn spawn_docked_panel(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
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
            Visibility::Hidden,
            BackgroundColor(Color::srgba(0.10, 0.10, 0.12, 0.92)),
            BorderColor::all(Color::srgb(0.38, 0.34, 0.22)),
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
                    BackgroundColor(Color::srgb(0.13, 0.12, 0.10)),
                ))
                .with_children(|title_row| {
                    title_row.spawn((
                        Text::new(""),
                        DockedPanelTitle { panel_id },
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.95, 0.89, 0.72)),
                    ));

                    title_row
                        .spawn((
                            Button,
                            DockedPanelCloseButton { panel_id },
                            Node {
                                width: px(22.0),
                                height: px(22.0),
                                border: UiRect::all(px(1.0)),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BorderColor::all(Color::srgb(0.52, 0.30, 0.20)),
                            BackgroundColor(Color::srgb(0.22, 0.11, 0.10)),
                        ))
                        .with_children(|button| {
                            button.spawn((
                                Text::new("x"),
                                TextFont {
                                    font_size: 14.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.94, 0.82, 0.74)),
                            ));
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
                    height: px(8.0),
                    ..default()
                },
                DockedPanelResizeHandle { panel_id },
                BackgroundColor(Color::srgb(0.18, 0.16, 0.12)),
            ));
        });
}

fn spawn_panel_label(parent: &mut ChildSpawnerCommands, label: &str) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font_size: 18.0,
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.89, 0.72)),
    ));
}

fn spawn_vital_bar<T: Component>(
    parent: &mut ChildSpawnerCommands,
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
                TextColor(Color::srgb(0.86, 0.84, 0.78)),
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
                    BackgroundColor(Color::srgb(0.18, 0.18, 0.20)),
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

fn spawn_slot_row(parent: &mut ChildSpawnerCommands, labels: &[&str]) {
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
                spawn_equipment_slot(row, Some(label));
            }
        });
}

fn spawn_equipment_slot(parent: &mut ChildSpawnerCommands, label: Option<&str>) {
    let slot = EquipmentSlot::ALL
        .into_iter()
        .find(|slot| Some(slot.label()) == label)
        .expect("Unknown equipment slot label");

    parent
        .spawn((
            Button,
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
            BorderColor::all(Color::srgb(0.38, 0.34, 0.22)),
            BackgroundColor(Color::srgb(0.16, 0.15, 0.12)),
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
                    TextColor(Color::srgb(0.80, 0.77, 0.69)),
                ));
            }
        });
}

fn spawn_backpack_slot(parent: &mut ChildSpawnerCommands, index: usize) {
    parent
        .spawn((
            Button,
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
                ..default()
            },
            BorderColor::all(Color::srgb(0.38, 0.34, 0.22)),
            BackgroundColor(Color::srgb(0.16, 0.15, 0.12)),
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
        });
}

fn spawn_open_container_slot(
    parent: &mut ChildSpawnerCommands,
    panel_id: usize,
    slot_index: usize,
) {
    parent
        .spawn((
            Button,
            ContainerSlotButton,
            ItemSlotButton {
                kind: ItemSlotKind::OpenContainer {
                    panel_id,
                    slot_index,
                },
            },
            Node {
                width: px(42.0),
                height: px(42.0),
                border: UiRect::all(px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BorderColor::all(Color::srgb(0.38, 0.34, 0.22)),
            BackgroundColor(Color::srgb(0.16, 0.15, 0.12)),
        ))
        .with_children(|slot| {
            slot.spawn((
                Node {
                    width: px(28.0),
                    height: px(28.0),
                    ..default()
                },
                ImageNode::default(),
                ItemSlotImage {
                    kind: ItemSlotKind::OpenContainer {
                        panel_id,
                        slot_index,
                    },
                },
                ContainerSlotImage,
                Visibility::Hidden,
            ));
        });
}

fn spawn_context_button<T: Component>(parent: &mut ChildSpawnerCommands, label: &str, marker: T) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                width: percent(100.0),
                min_height: px(28.0),
                padding: UiRect::axes(px(8.0), px(4.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.18, 0.15, 0.11)),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(Color::srgb(0.94, 0.88, 0.72)),
            ));
        });
}
