//! Toggleable side panel that lists the current map's vendor stashes —
//! named ware lists that placed shopkeeper NPCs can reference via the
//! `vendor_stash` property to override their template's default wares.
//!
//! Visibility is driven by `EditorState::vendor_stashes_panel_visible`.
//! Selecting a stash expands it inline; each ware row exposes type_id, price,
//! and stock as click-to-edit text fields. Keyboard input is handled by
//! `handle_vendor_stash_keyboard_input`, which mirrors the property panel's
//! commit/cancel flow.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::editor::resources::{
    EditorState, EditorVendorStashBuffer, VendorStashEditingField, VendorWarePickTarget,
};
use crate::editor::ui::palette::EditorPaletteItem;
use crate::game::shop::{StockModeDef, StockWord, WareDef};
use crate::world::map_layout::VendorStashDef;
use crate::world::object_definitions::OverworldObjectDefinitions;

#[derive(Component)]
pub struct EditorVendorStashesRoot;

#[derive(Component)]
pub struct EditorVendorStashesContent;

#[derive(Component, Clone, Copy)]
pub struct EditorVendorStashRow {
    pub index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct EditorVendorStashDeleteButton {
    pub index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct EditorVendorStashDuplicateButton {
    pub index: usize,
}

#[derive(Component)]
pub struct EditorVendorStashAddButton;

/// Inline-edit hot zones. Each one carries enough context for the click
/// handler to drop the corresponding `VendorStashEditingField` into the
/// buffer and seed `edit_text` with the existing value.
#[derive(Component, Clone, Copy)]
pub struct EditorVendorStashIdField {
    pub stash_index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct EditorVendorStashWareField {
    pub stash_index: usize,
    pub ware_index: usize,
    pub kind: WareFieldKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WareFieldKind {
    TypeId,
    Price,
    Stock,
}

#[derive(Component, Clone, Copy)]
pub struct EditorVendorStashWareDeleteButton {
    pub stash_index: usize,
    pub ware_index: usize,
}

#[derive(Component, Clone, Copy)]
pub struct EditorVendorStashAddWareButton {
    pub stash_index: usize,
}

const PANEL_WIDTH_PX: f32 = 280.0;

pub fn spawn_vendor_stashes_panel(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            EditorVendorStashesRoot,
            Node {
                width: Val::Px(PANEL_WIDTH_PX),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::left(Val::Px(1.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.04, 0.04, 0.92)),
            BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Vendor Stashes"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.96, 0.84, 0.62)),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    h.spawn((
                        Button,
                        EditorVendorStashAddButton,
                        Node {
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.18, 0.12, 0.06, 0.95)),
                        BorderColor::all(Color::srgb(0.55, 0.40, 0.22)),
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("+ Add"),
                            TextFont {
                                font_size: 12.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.96, 0.86, 0.66)),
                        ));
                    });
                });

            panel.spawn((
                EditorVendorStashesContent,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                bevy::ui::ScrollPosition::default(),
            ));
        });
}

pub fn sync_vendor_stashes_panel_visibility(
    editor_state: Res<EditorState>,
    mut roots: Query<&mut Node, With<EditorVendorStashesRoot>>,
) {
    if !editor_state.is_changed() {
        return;
    }
    let target = if editor_state.vendor_stashes_panel_visible {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut roots {
        if node.display != target {
            node.display = target;
        }
    }
}

/// Rebuild rows whenever the buffer or visibility flag changes.
pub fn sync_vendor_stashes_panel(
    editor_state: Res<EditorState>,
    buffer: Res<EditorVendorStashBuffer>,
    definitions: Res<OverworldObjectDefinitions>,
    content: Query<Entity, With<EditorVendorStashesContent>>,
    rows: Query<Entity, With<EditorVendorStashRow>>,
    mut commands: Commands,
) {
    if !editor_state.vendor_stashes_panel_visible {
        return;
    }
    if !buffer.is_changed() && !editor_state.is_changed() {
        return;
    }

    for row in &rows {
        commands.entity(row).despawn();
    }
    let Ok(content_entity) = content.single() else {
        return;
    };

    if buffer.stashes.is_empty() {
        commands.entity(content_entity).with_children(|c| {
            c.spawn((
                EditorVendorStashRow { index: usize::MAX },
                Text::new("(no vendor stashes — click + Add)"),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.50, 0.46, 0.42)),
                Node {
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                },
            ));
        });
        return;
    }

    let selected = buffer.selected;
    let editing = buffer.editing;
    let edit_text = buffer.edit_text.clone();
    let pending_pick = buffer.pending_ware_pick;

    commands.entity(content_entity).with_children(|c| {
        for (index, stash) in buffer.stashes.iter().enumerate() {
            let is_selected = selected == Some(index);
            let id_display = if matches!(
                editing,
                Some(VendorStashEditingField::StashId { stash_index }) if stash_index == index
            ) {
                format!("[{}]", edit_text)
            } else {
                stash.id.clone()
            };

            c.spawn((
                EditorVendorStashRow { index },
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    row_gap: Val::Px(3.0),
                    ..default()
                },
                BackgroundColor(if is_selected {
                    Color::srgba(0.20, 0.14, 0.08, 0.95)
                } else {
                    Color::srgba(0.10, 0.07, 0.06, 0.80)
                }),
                BorderColor::all(if is_selected {
                    Color::srgb(0.85, 0.65, 0.30)
                } else {
                    Color::srgb(0.20, 0.15, 0.10)
                }),
            ))
            .with_children(|row| {
                // Header row: id (click-to-edit) + ware count + duplicate/delete.
                row.spawn((Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                },))
                    .with_children(|header| {
                        header
                            .spawn((
                                Button,
                                EditorVendorStashIdField { stash_index: index },
                                Node {
                                    flex_grow: 1.0,
                                    padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                                    ..default()
                                },
                            ))
                            .with_children(|f| {
                                f.spawn((
                                    Text::new(id_display),
                                    TextFont {
                                        font_size: 12.0,
                                        ..default()
                                    },
                                    TextColor(Color::srgb(0.96, 0.86, 0.66)),
                                ));
                            });
                        header.spawn((
                            Text::new(format!("({} wares)", stash.wares.len())),
                            TextFont {
                                font_size: 10.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.70, 0.64, 0.55)),
                        ));
                        action_button(header, "Dup", EditorVendorStashDuplicateButton { index });
                        action_button(header, "Del", EditorVendorStashDeleteButton { index });
                    });

                if !is_selected {
                    return;
                }

                // Expanded wares editor.
                row.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        margin: UiRect::top(Val::Px(4.0)),
                        padding: UiRect::top(Val::Px(4.0)),
                        border: UiRect::top(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.25, 0.18, 0.12)),
                ))
                .with_children(|wares_section| {
                    if stash.wares.is_empty() {
                        wares_section.spawn((
                            Text::new("(no wares)"),
                            TextFont {
                                font_size: 10.0,
                                ..default()
                            },
                            TextColor(Color::srgb(0.50, 0.46, 0.42)),
                        ));
                    }
                    for (ware_index, ware) in stash.wares.iter().enumerate() {
                        // The type_id column is read-only text driven by
                        // palette pick — display the picked item's *display
                        // name* (or the raw id when no def matches), with a
                        // visible prompt when this row is the pending pick
                        // target.
                        let pick_armed = pending_pick
                            == Some(VendorWarePickTarget {
                                stash_index: index,
                                ware_index,
                            });
                        let type_id_text = if pick_armed {
                            "← pick from palette".to_owned()
                        } else if ware.type_id.is_empty() {
                            "(click to pick)".to_owned()
                        } else {
                            definitions
                                .get(&ware.type_id)
                                .map(|d| format!("{} ({})", d.name, ware.type_id))
                                .unwrap_or_else(|| ware.type_id.clone())
                        };
                        let price_text = ware_field_text(
                            &edit_text,
                            editing,
                            index,
                            ware_index,
                            WareFieldKind::Price,
                            || ware.price_copper.to_string(),
                        );
                        let stock_text = ware_field_text(
                            &edit_text,
                            editing,
                            index,
                            ware_index,
                            WareFieldKind::Stock,
                            || format_stock(&ware.stock),
                        );
                        wares_section
                            .spawn((Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Row,
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(4.0),
                                ..default()
                            },))
                            .with_children(|ware_row| {
                                ware_field_button(
                                    ware_row,
                                    type_id_text,
                                    EditorVendorStashWareField {
                                        stash_index: index,
                                        ware_index,
                                        kind: WareFieldKind::TypeId,
                                    },
                                    2.6,
                                    pick_armed,
                                );
                                ware_field_button(
                                    ware_row,
                                    price_text,
                                    EditorVendorStashWareField {
                                        stash_index: index,
                                        ware_index,
                                        kind: WareFieldKind::Price,
                                    },
                                    1.0,
                                    false,
                                );
                                ware_field_button(
                                    ware_row,
                                    stock_text,
                                    EditorVendorStashWareField {
                                        stash_index: index,
                                        ware_index,
                                        kind: WareFieldKind::Stock,
                                    },
                                    1.2,
                                    false,
                                );
                                action_button(
                                    ware_row,
                                    "×",
                                    EditorVendorStashWareDeleteButton {
                                        stash_index: index,
                                        ware_index,
                                    },
                                );
                            });
                    }
                    wares_section
                        .spawn((
                            Button,
                            EditorVendorStashAddWareButton { stash_index: index },
                            Node {
                                width: Val::Percent(100.0),
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                                justify_content: JustifyContent::Center,
                                border: UiRect::all(Val::Px(1.0)),
                                margin: UiRect::top(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.12, 0.09, 0.06, 0.80)),
                            BorderColor::all(Color::srgb(0.30, 0.22, 0.14)),
                        ))
                        .with_children(|btn| {
                            btn.spawn((
                                Text::new("+ Add ware"),
                                TextFont {
                                    font_size: 10.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.85, 0.80, 0.70)),
                            ));
                        });
                });
            });
        }
    });
}

fn ware_field_text(
    edit_text: &str,
    editing: Option<VendorStashEditingField>,
    stash_index: usize,
    ware_index: usize,
    kind: WareFieldKind,
    default: impl FnOnce() -> String,
) -> String {
    let active = match (editing, kind) {
        (
            Some(VendorStashEditingField::WareTypeId {
                stash_index: si,
                ware_index: wi,
            }),
            WareFieldKind::TypeId,
        )
        | (
            Some(VendorStashEditingField::WarePrice {
                stash_index: si,
                ware_index: wi,
            }),
            WareFieldKind::Price,
        )
        | (
            Some(VendorStashEditingField::WareStock {
                stash_index: si,
                ware_index: wi,
            }),
            WareFieldKind::Stock,
        ) => si == stash_index && wi == ware_index,
        _ => false,
    };
    if active {
        format!("[{}]", edit_text)
    } else {
        default()
    }
}

fn format_stock(stock: &StockModeDef) -> String {
    match stock {
        StockModeDef::Word(StockWord::Infinite) => "infinite".to_owned(),
        StockModeDef::Count(n) => n.to_string(),
    }
}

fn ware_field_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    label: String,
    marker: M,
    flex_grow: f32,
    highlighted: bool,
) {
    let (bg, border, text_color) = if highlighted {
        (
            Color::srgba(0.28, 0.18, 0.06, 0.95),
            Color::srgb(0.95, 0.78, 0.30),
            Color::srgb(1.00, 0.92, 0.60),
        )
    } else {
        (
            Color::srgba(0.10, 0.07, 0.06, 0.80),
            Color::srgb(0.25, 0.18, 0.12),
            Color::srgb(0.92, 0.86, 0.74),
        )
    };
    parent
        .spawn((
            Button,
            marker,
            Node {
                flex_grow,
                flex_basis: Val::Px(0.0),
                min_width: Val::Px(0.0),
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(bg),
            BorderColor::all(border),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(text_color),
                Node {
                    overflow: Overflow::clip_x(),
                    ..default()
                },
            ));
        });
}

fn action_button<M: Component>(parent: &mut ChildSpawnerCommands, label: &str, marker: M) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.14, 0.10, 0.08, 0.95)),
            BorderColor::all(Color::srgb(0.40, 0.30, 0.20)),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.92, 0.86, 0.74)),
            ));
        });
}

/// Click handlers: row select, action buttons, add button, field-edit kick-off.
#[allow(clippy::too_many_arguments)]
pub fn handle_vendor_stashes_panel_clicks(
    rows: Query<
        (&EditorVendorStashRow, &Interaction),
        (Changed<Interaction>, With<EditorVendorStashRow>),
    >,
    delete_btns: Query<
        (&EditorVendorStashDeleteButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    duplicate_btns: Query<
        (&EditorVendorStashDuplicateButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    add_btn: Query<&Interaction, (Changed<Interaction>, With<EditorVendorStashAddButton>)>,
    id_fields: Query<
        (&EditorVendorStashIdField, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    ware_fields: Query<
        (&EditorVendorStashWareField, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    add_ware_btns: Query<
        (&EditorVendorStashAddWareButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    ware_del_btns: Query<
        (&EditorVendorStashWareDeleteButton, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
    mut buffer: ResMut<EditorVendorStashBuffer>,
    mut editor_state: ResMut<EditorState>,
    mut prop_buffer: ResMut<crate::editor::resources::EditorPropertyEditBuffer>,
) {
    // Row toggles selection (expand / collapse the wares editor).
    for (row, interaction) in &rows {
        if *interaction == Interaction::Pressed && row.index != usize::MAX {
            commit_active_edit(&mut buffer);
            buffer.selected = if buffer.selected == Some(row.index) {
                None
            } else {
                Some(row.index)
            };
        }
    }

    for (btn, interaction) in &delete_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if btn.index >= buffer.stashes.len() {
            continue;
        }
        commit_active_edit(&mut buffer);
        buffer.stashes.remove(btn.index);
        if buffer.selected == Some(btn.index) {
            buffer.selected = None;
        } else if let Some(s) = buffer.selected {
            if s > btn.index {
                buffer.selected = Some(s - 1);
            }
        }
        editor_state.dirty = true;
    }

    for (btn, interaction) in &duplicate_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(source) = buffer.stashes.get(btn.index).cloned() else {
            continue;
        };
        commit_active_edit(&mut buffer);
        let new_id = unique_clone_id(&source.id, &buffer.stashes);
        let new_index = buffer.stashes.len();
        buffer.stashes.push(VendorStashDef {
            id: new_id,
            wares: source.wares,
        });
        buffer.selected = Some(new_index);
        editor_state.dirty = true;
    }

    for interaction in &add_btn {
        if *interaction != Interaction::Pressed {
            continue;
        }
        commit_active_edit(&mut buffer);
        let new_id = unique_new_id(&buffer.stashes);
        let new_index = buffer.stashes.len();
        buffer.stashes.push(VendorStashDef {
            id: new_id,
            wares: Vec::new(),
        });
        buffer.selected = Some(new_index);
        editor_state.dirty = true;
    }

    for (field, interaction) in &id_fields {
        if *interaction != Interaction::Pressed {
            continue;
        }
        commit_active_edit(&mut buffer);
        // Clear competing property-panel edit to keep the keyboard pipeline
        // single-owner.
        prop_buffer.editing_index = None;
        prop_buffer.edit_text.clear();
        let initial = buffer
            .stashes
            .get(field.stash_index)
            .map(|s| s.id.clone())
            .unwrap_or_default();
        buffer.editing = Some(VendorStashEditingField::StashId {
            stash_index: field.stash_index,
        });
        buffer.edit_text = initial;
        buffer.selected = Some(field.stash_index);
        buffer.pending_ware_pick = None;
    }

    for (field, interaction) in &ware_fields {
        if *interaction != Interaction::Pressed {
            continue;
        }
        commit_active_edit(&mut buffer);
        prop_buffer.editing_index = None;
        prop_buffer.edit_text.clear();
        // type_id is a palette-pick target, not a text field — clicking it
        // arms the next palette click to fill the field. Clicking the same
        // type_id cell again cancels the arm. This avoids the truly bad UX
        // of typing raw object ids.
        if field.kind == WareFieldKind::TypeId {
            let target = VendorWarePickTarget {
                stash_index: field.stash_index,
                ware_index: field.ware_index,
            };
            buffer.pending_ware_pick = if buffer.pending_ware_pick == Some(target) {
                None
            } else {
                Some(target)
            };
            buffer.selected = Some(field.stash_index);
            continue;
        }
        let Some(ware) = buffer
            .stashes
            .get(field.stash_index)
            .and_then(|s| s.wares.get(field.ware_index))
        else {
            continue;
        };
        let (editing, initial) = match field.kind {
            WareFieldKind::TypeId => unreachable!("handled above"),
            WareFieldKind::Price => (
                VendorStashEditingField::WarePrice {
                    stash_index: field.stash_index,
                    ware_index: field.ware_index,
                },
                ware.price_copper.to_string(),
            ),
            WareFieldKind::Stock => (
                VendorStashEditingField::WareStock {
                    stash_index: field.stash_index,
                    ware_index: field.ware_index,
                },
                format_stock(&ware.stock),
            ),
        };
        buffer.editing = Some(editing);
        buffer.edit_text = initial;
        buffer.selected = Some(field.stash_index);
        buffer.pending_ware_pick = None;
    }

    for (btn, interaction) in &add_ware_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        commit_active_edit(&mut buffer);
        let Some(stash) = buffer.stashes.get_mut(btn.stash_index) else {
            continue;
        };
        let ware_index = stash.wares.len();
        stash.wares.push(WareDef {
            type_id: String::new(),
            price_copper: 0,
            stock: StockModeDef::default(),
        });
        prop_buffer.editing_index = None;
        prop_buffer.edit_text.clear();
        buffer.editing = None;
        buffer.edit_text.clear();
        // Arm the type_id pick on the freshly-added ware so the user's next
        // palette click fills it in. Far better UX than landing in a blank
        // text field and expecting the user to remember the type_id by name.
        buffer.pending_ware_pick = Some(VendorWarePickTarget {
            stash_index: btn.stash_index,
            ware_index,
        });
        buffer.selected = Some(btn.stash_index);
        editor_state.dirty = true;
    }

    for (btn, interaction) in &ware_del_btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // If the deleted ware was being edited, drop the edit first so we
        // don't end up pointing the edit cursor at a now-empty slot.
        let editing_matches_this_ware = match buffer.editing {
            Some(
                VendorStashEditingField::WareTypeId {
                    stash_index,
                    ware_index,
                }
                | VendorStashEditingField::WarePrice {
                    stash_index,
                    ware_index,
                }
                | VendorStashEditingField::WareStock {
                    stash_index,
                    ware_index,
                },
            ) => stash_index == btn.stash_index && ware_index == btn.ware_index,
            _ => false,
        };
        if editing_matches_this_ware {
            buffer.editing = None;
            buffer.edit_text.clear();
        }
        if buffer.pending_ware_pick
            == Some(VendorWarePickTarget {
                stash_index: btn.stash_index,
                ware_index: btn.ware_index,
            })
        {
            buffer.pending_ware_pick = None;
        }
        let Some(stash) = buffer.stashes.get_mut(btn.stash_index) else {
            continue;
        };
        if btn.ware_index >= stash.wares.len() {
            continue;
        }
        stash.wares.remove(btn.ware_index);
        editor_state.dirty = true;
    }
}

/// Keyboard input pipeline for the active vendor-stash edit field. Mirrors
/// `handle_editor_keyboard_input` so Esc / Enter / Backspace / characters do
/// the expected thing without clashing with the property panel — `editing`
/// is held in a different buffer and the two are kept mutually exclusive by
/// their click handlers.
pub fn handle_vendor_stash_keyboard_input(
    mut keyboard_events: bevy::ecs::message::MessageReader<KeyboardInput>,
    mut buffer: ResMut<EditorVendorStashBuffer>,
    mut editor_state: ResMut<EditorState>,
) {
    // Esc also cancels a pending palette-pick, even when no text field is
    // being edited — otherwise the armed pick state could only be cleared by
    // re-clicking the same cell.
    if buffer.editing.is_none() {
        if buffer.pending_ware_pick.is_some() {
            for event in keyboard_events.read() {
                if event.state.is_pressed() && event.key_code == KeyCode::Escape {
                    buffer.pending_ware_pick = None;
                }
            }
        }
        return;
    }
    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match event.key_code {
            KeyCode::Escape => {
                buffer.editing = None;
                buffer.edit_text.clear();
            }
            KeyCode::Enter | KeyCode::Tab => {
                commit_active_edit(&mut buffer);
                editor_state.dirty = true;
            }
            KeyCode::Backspace => {
                buffer.edit_text.pop();
            }
            _ => {
                if event.repeat {
                    continue;
                }
                match &event.logical_key {
                    Key::Character(ch) => {
                        buffer.edit_text.push_str(ch.as_str());
                    }
                    Key::Space => {
                        buffer.edit_text.push(' ');
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Commit whatever's buffered in `edit_text` to the matching field. No-op
/// when no field is being edited. Called whenever focus is about to move
/// (Tab/Enter, clicking another field, deleting/duplicating a stash).
fn commit_active_edit(buffer: &mut EditorVendorStashBuffer) {
    let Some(editing) = buffer.editing else {
        return;
    };
    let text = std::mem::take(&mut buffer.edit_text);
    match editing {
        VendorStashEditingField::StashId { stash_index } => {
            if let Some(stash) = buffer.stashes.get_mut(stash_index) {
                stash.id = text.trim().to_owned();
            }
        }
        VendorStashEditingField::WareTypeId { .. } => {
            // Defensive — the type_id field is palette-pick only and never
            // enters text-edit mode in normal flow. If something armed this
            // variant (e.g. a stale buffer), just drop the buffered text.
        }
        VendorStashEditingField::WarePrice {
            stash_index,
            ware_index,
        } => {
            if let Some(ware) = buffer
                .stashes
                .get_mut(stash_index)
                .and_then(|s| s.wares.get_mut(ware_index))
            {
                ware.price_copper = text.trim().parse::<u32>().unwrap_or(ware.price_copper);
            }
        }
        VendorStashEditingField::WareStock {
            stash_index,
            ware_index,
        } => {
            if let Some(ware) = buffer
                .stashes
                .get_mut(stash_index)
                .and_then(|s| s.wares.get_mut(ware_index))
            {
                ware.stock = parse_stock(&text);
            }
        }
    }
    buffer.editing = None;
}

/// Intercept palette clicks when a vendor-stash pick is armed: the picked
/// item's `type_id` fills the target ware and the arm clears. Ordered
/// *after* `handle_palette_clicks` (which bails when a pick is pending)
/// so the brush isn't accidentally armed by the same click.
pub fn handle_vendor_stash_palette_pick(
    items: Query<(&EditorPaletteItem, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut buffer: ResMut<EditorVendorStashBuffer>,
    mut editor_state: ResMut<EditorState>,
) {
    let Some(target) = buffer.pending_ware_pick else {
        return;
    };
    for (item, interaction) in &items {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Some(ware) = buffer
            .stashes
            .get_mut(target.stash_index)
            .and_then(|s| s.wares.get_mut(target.ware_index))
        {
            ware.type_id = item.type_id.clone();
            editor_state.dirty = true;
        }
        buffer.pending_ware_pick = None;
        break;
    }
}

fn parse_stock(text: &str) -> StockModeDef {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("infinite") || trimmed.eq_ignore_ascii_case("inf") {
        StockModeDef::Word(StockWord::Infinite)
    } else if let Ok(n) = trimmed.parse::<u32>() {
        StockModeDef::Count(n)
    } else {
        // Garbage input keeps the prior intent: treat as infinite so the
        // YAML stays valid. (Numeric parse errors mostly come from a half-
        // typed digit; rather than silently dropping to 0 we fall back to
        // the safer default.)
        StockModeDef::default()
    }
}

fn unique_new_id(stashes: &[VendorStashDef]) -> String {
    let mut suffix = 1;
    loop {
        let candidate = format!("stash_{suffix}");
        if !stashes.iter().any(|s| s.id == candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

fn unique_clone_id(base: &str, stashes: &[VendorStashDef]) -> String {
    let mut candidate = format!("{base}_copy");
    let mut suffix = 2;
    while stashes.iter().any(|s| s.id == candidate) {
        candidate = format!("{base}_copy{suffix}");
        suffix += 1;
    }
    candidate
}
