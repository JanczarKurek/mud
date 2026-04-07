use bevy::prelude::*;
use bevy::text::{Justify, LineBreak, TextLayout};

use crate::ui::components::{
    ChatLogText, CloseContainerButton, ContainerSlotButton, ContainerSlotImage,
    ContextMenuInspectButton, ContextMenuOpenButton, ContextMenuRoot, ContextMenuUseButton,
    ContextMenuUseOnButton, DragPreviewLabel, DragPreviewRoot, EquipmentSlotButton,
    EquipmentSlotImage, HealthFill, HealthLabel, ItemSlotButton, ItemSlotImage, ItemSlotKind,
    ManaFill, ManaLabel, OpenContainerTitle, PythonConsoleInput, PythonConsoleOutput,
    PythonConsoleOutputViewport, PythonConsolePanel, PythonConsoleScrollbarThumb,
};
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
                        width: px(320.0),
                        height: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::SpaceBetween,
                        padding: UiRect::axes(px(16.0), px(16.0)),
                        row_gap: px(16.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.06, 0.06, 0.08, 0.88)),
                ))
                .with_children(|sidebar| {
                    sidebar
                        .spawn((
                            Node {
                                width: percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(12.0),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|top_panel| {
                            spawn_panel_label(top_panel, "Status");
                            spawn_vital_bar(
                                top_panel,
                                "Health",
                                Color::srgb(0.70, 0.16, 0.18),
                                HealthFill,
                                HealthLabel,
                            );
                            spawn_vital_bar(
                                top_panel,
                                "Mana",
                                Color::srgb(0.14, 0.35, 0.78),
                                ManaFill,
                                ManaLabel,
                            );
                        });

                    sidebar
                        .spawn((
                            Node {
                                width: percent(100.0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                row_gap: px(12.0),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|inventory_panel| {
                            spawn_equipment_panel(inventory_panel);
                            spawn_open_container_panel(inventory_panel);
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
            spawn_context_button(menu, "Use", ContextMenuUseButton);
            spawn_context_button(menu, "Use On", ContextMenuUseOnButton);
            spawn_context_button(menu, "Inspect", ContextMenuInspectButton);
            spawn_context_button(menu, "Open", ContextMenuOpenButton);
        });
}

fn spawn_equipment_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(12.0),
                padding: UiRect::all(px(12.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.10, 0.10, 0.12, 0.92)),
        ))
        .with_children(|equipment_panel| {
            spawn_panel_label(equipment_panel, "Equipment");

            equipment_panel
                .spawn((
                    Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },
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

fn spawn_open_container_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(12.0),
                padding: UiRect::all(px(12.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.10, 0.10, 0.12, 0.92)),
        ))
        .with_children(|container_panel| {
            container_panel
                .spawn((
                    Node {
                        width: percent(100.0),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|title_row| {
                    title_row.spawn((
                        Text::new("Backpack"),
                        OpenContainerTitle,
                        TextFont {
                            font_size: 24.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.95, 0.89, 0.72)),
                    ));

                    title_row
                        .spawn((
                            Button,
                            CloseContainerButton,
                            Node {
                                width: px(28.0),
                                height: px(28.0),
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
                                    font_size: 18.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.94, 0.82, 0.74)),
                            ));
                        });
                });

            container_panel
                .spawn((
                    Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8.0),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|grid| {
                    for row_index in 0..4 {
                        grid.spawn((
                            Node {
                                width: percent(100.0),
                                column_gap: px(8.0),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|row| {
                            for column in 0..4 {
                                let index = row_index * 4 + column;
                                spawn_container_slot(row, index);
                            }
                        });
                    }
                });
        });
}

fn spawn_panel_label(parent: &mut ChildSpawnerCommands, label: &str) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font_size: 24.0,
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
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .with_children(|bar_group| {
            bar_group.spawn((
                Text::new(label),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(Color::srgb(0.86, 0.84, 0.78)),
            ));

            bar_group
                .spawn((
                    Node {
                        width: percent(100.0),
                        height: px(24.0),
                        padding: UiRect::all(px(3.0)),
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
                    bar_container.spawn((
                        Text::new(""),
                        value_marker,
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.95, 0.92)),
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(50.0),
                            ..default()
                        },
                    ));
                });
        });
}

fn spawn_slot_row(parent: &mut ChildSpawnerCommands, labels: &[&str]) {
    parent
        .spawn((
            Node {
                column_gap: px(8.0),
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
                width: px(58.0),
                height: px(58.0),
                border: UiRect::all(px(2.0)),
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
                    width: px(42.0),
                    height: px(42.0),
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
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.80, 0.77, 0.69)),
                ));
            }
        });
}

fn spawn_container_slot(parent: &mut ChildSpawnerCommands, index: usize) {
    parent
        .spawn((
            Button,
            ContainerSlotButton,
            ItemSlotButton {
                kind: ItemSlotKind::ActiveContainer(index),
            },
            Node {
                width: px(58.0),
                height: px(58.0),
                border: UiRect::all(px(2.0)),
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
                    width: px(42.0),
                    height: px(42.0),
                    ..default()
                },
                ImageNode::default(),
                ItemSlotImage {
                    kind: ItemSlotKind::ActiveContainer(index),
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
