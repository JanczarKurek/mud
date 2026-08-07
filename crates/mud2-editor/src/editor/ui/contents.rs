//! Editor "Contents" section: view and edit what's inside a selected
//! container (chest / barrel / crate / pouch), including one level of nesting.
//!
//! The section is rendered inside `sync_properties_panel` (properties.rs) when
//! the selected object's definition declares `container_capacity`. The slots
//! themselves are the single source of truth on the entity's `Container`
//! component; `EditorContentsBuffer` holds only transient edit / pick state.
//!
//! Item picking reuses the vendor-stash palette-click arm: clicking "+ Add
//! item" (or a slot's type cell) arms `pending_item_pick`, and the next click
//! on a palette item is captured by `handle_contents_palette_pick` instead of
//! arming the object brush (gated in `handle_palette_clicks`).

use bevy::prelude::*;

use crate::editor::resources::{
    ContainerRef, ContentsEditTarget, ContentsPickTarget, EditorContentsBuffer,
    EditorPropertyEditBuffer, EditorStackEdit, EditorState, EditorVendorStashBuffer, SlotAddr,
};
use crate::editor::ui::palette::EditorPaletteItem;
use mud2::player::components::InventoryStack;
use mud2::ui::theme::text_field::{
    drive_inline_edit_keyboard, inline_edit_display, InlineEditState,
};
use mud2::world::components::{Container, OverworldObject};
use mud2::world::map_layout::ObjectProperties;
use mud2::world::object_definitions::OverworldObjectDefinitions;

// ── Component markers ────────────────────────────────────────────────────────

/// "+ Add item" button; adds to the container it names.
#[derive(Component, Clone, Copy)]
pub struct EditorContentsAddButton {
    pub container: ContainerRef,
}

/// A slot's type cell; clicking arms a retype pick for that slot.
#[derive(Component, Clone, Copy)]
pub struct EditorContentsTypeCell {
    pub addr: SlotAddr,
}

/// A slot's stack-count cell; clicking starts a quantity edit.
#[derive(Component, Clone, Copy)]
pub struct EditorContentsQtyCell {
    pub addr: SlotAddr,
}

/// A slot's remove button.
#[derive(Component, Clone, Copy)]
pub struct EditorContentsRemoveButton {
    pub addr: SlotAddr,
}

/// Expand / collapse caret on a top-level slot that is itself a container.
#[derive(Component, Clone, Copy)]
pub struct EditorContentsExpandButton {
    pub top: usize,
}

/// A property row under a slot item; clicking edits its value.
#[derive(Component, Clone, Copy)]
pub struct EditorContentsPropRow {
    pub addr: SlotAddr,
    pub prop_index: usize,
}

/// "+ prop" button under a slot item; adds a blank property and edits the key.
#[derive(Component, Clone, Copy)]
pub struct EditorContentsAddPropButton {
    pub addr: SlotAddr,
}

// ── Slot access helpers ──────────────────────────────────────────────────────

/// Immutable view of the slot list for the given container ref within `c`.
pub fn container_slots(c: &Container, cref: ContainerRef) -> Option<&[Option<InventoryStack>]> {
    match cref {
        None => Some(&c.slots),
        Some(top) => c
            .slots
            .get(top)
            .and_then(|s| s.as_ref())
            .and_then(|stack| stack.contained_slots.as_deref()),
    }
}

/// Mutable slot list for the given container ref within `c`.
fn container_slots_mut(
    c: &mut Container,
    cref: ContainerRef,
) -> Option<&mut Vec<Option<InventoryStack>>> {
    match cref {
        None => Some(&mut c.slots),
        Some(top) => c
            .slots
            .get_mut(top)
            .and_then(|s| s.as_mut())
            .and_then(|stack| stack.contained_slots.as_mut()),
    }
}

/// Sorted `(key, value)` view of a slot item's properties (stable display order).
fn sorted_props(stack: &InventoryStack) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = stack
        .properties
        .iter()
        .map(|(k, val)| (k.clone(), val.clone()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

// ── Rendering ────────────────────────────────────────────────────────────────

const HDR: Color = crate::editor::ui::style::HEADER_TEXT;
const DIM: Color = Color::srgb(0.70, 0.66, 0.60);
const ROW_BG: Color = Color::srgba(0.10, 0.08, 0.06, 0.70);
const ROW_BG_EDIT: Color = Color::srgba(0.20, 0.15, 0.08, 0.90);
const ROW_BORDER: Color = Color::srgb(0.22, 0.16, 0.12);
const ROW_BORDER_EDIT: Color = Color::srgb(0.90, 0.72, 0.40);

/// Render the whole Contents section under the current object's Container.
pub fn spawn_contents_section(
    parent: &mut ChildSpawnerCommands,
    capacity: usize,
    container: &Container,
    definitions: &OverworldObjectDefinitions,
    buffer: &EditorContentsBuffer,
) {
    let used = container.slots.iter().flatten().count();
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                margin: UiRect::top(Val::Px(8.0)),
                border: UiRect::top(Val::Px(1.0)),
                padding: UiRect::top(Val::Px(6.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                ..default()
            },
            BorderColor::all(Color::srgb(0.30, 0.22, 0.14)),
        ))
        .with_children(|section| {
            section.spawn((
                Text::new(format!("Contents ({used}/{capacity})")),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(HDR),
            ));

            for (i, slot) in container.slots.iter().enumerate() {
                let Some(stack) = slot else { continue };
                let addr = SlotAddr {
                    container: None,
                    index: i,
                };
                let nested_cap = definitions
                    .get(&stack.type_id)
                    .and_then(|d| d.container_capacity);
                spawn_item_row(section, addr, stack, nested_cap.is_some(), buffer);

                if nested_cap.is_some() && buffer.expanded == Some(i) {
                    section
                        .spawn((
                            Node {
                                width: Val::Percent(100.0),
                                margin: UiRect::left(Val::Px(12.0)),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(3.0),
                                ..default()
                            },
                            BorderColor::all(ROW_BORDER),
                        ))
                        .with_children(|nested| {
                            let sub_slots = stack.contained_slots.as_deref().unwrap_or(&[]);
                            let sub_cap = nested_cap.unwrap_or(0);
                            let sub_used = sub_slots.iter().flatten().count();
                            for (j, sub) in sub_slots.iter().enumerate() {
                                let Some(sub_stack) = sub else { continue };
                                let sub_addr = SlotAddr {
                                    container: Some(i),
                                    index: j,
                                };
                                // A nested container never itself holds a
                                // container (pouches set
                                // `accepts_storable_containers: false`), so no
                                // expand caret at depth 2.
                                spawn_item_row(nested, sub_addr, sub_stack, false, buffer);
                            }
                            spawn_add_item_button(nested, Some(i), sub_used >= sub_cap);
                        });
                }
            }

            spawn_add_item_button(section, None, used >= capacity);
        });
}

fn spawn_item_row(
    parent: &mut ChildSpawnerCommands,
    addr: SlotAddr,
    stack: &InventoryStack,
    is_container: bool,
    buffer: &EditorContentsBuffer,
) {
    let editing_qty = buffer.editing == Some(ContentsEditTarget::Quantity { addr });
    let qty_text = if editing_qty {
        inline_edit_display(&buffer.edit_text)
    } else {
        format!("x{}", stack.quantity)
    };

    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(4.0),
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(ROW_BG),
            BorderColor::all(ROW_BORDER),
        ))
        .with_children(|row| {
            if is_container {
                row.spawn((
                    Button,
                    EditorContentsExpandButton { top: addr.index },
                    Node {
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Text::new(if buffer.expanded == Some(addr.index) {
                        "v"
                    } else {
                        ">"
                    }),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(DIM),
                ));
            }
            // Type cell (click → arm retype).
            row.spawn((
                Button,
                EditorContentsTypeCell { addr },
                Node {
                    flex_grow: 1.0,
                    overflow: Overflow::clip_x(),
                    ..default()
                },
                Text::new(stack.type_id.clone()),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.96, 0.92, 0.80)),
            ));
            // Quantity cell (click → edit).
            row.spawn((
                Button,
                EditorContentsQtyCell { addr },
                Node {
                    flex_shrink: 0.0,
                    padding: UiRect::horizontal(Val::Px(3.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(if editing_qty { ROW_BG_EDIT } else { ROW_BG }),
                BorderColor::all(if editing_qty {
                    ROW_BORDER_EDIT
                } else {
                    ROW_BORDER
                }),
                Text::new(qty_text),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.80, 0.86, 0.70)),
            ));
            // Remove button.
            row.spawn((
                Button,
                EditorContentsRemoveButton { addr },
                Node {
                    flex_shrink: 0.0,
                    padding: UiRect::horizontal(Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.20, 0.08, 0.06, 0.80)),
                BorderColor::all(Color::srgb(0.45, 0.20, 0.16)),
                Text::new("x"),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.90, 0.70, 0.66)),
            ));
        });

    // Property rows for this item.
    for (prop_index, (key, value)) in sorted_props(stack).into_iter().enumerate() {
        let editing_key =
            buffer.editing == Some(ContentsEditTarget::PropertyKey { addr, prop_index });
        let editing_value =
            buffer.editing == Some(ContentsEditTarget::PropertyValue { addr, prop_index });
        let shown_key = if editing_key {
            inline_edit_display(&buffer.edit_text)
        } else {
            key
        };
        let shown_value = if editing_value {
            inline_edit_display(&buffer.edit_text)
        } else {
            value
        };
        parent
            .spawn((
                Button,
                EditorContentsPropRow { addr, prop_index },
                Node {
                    width: Val::Percent(100.0),
                    margin: UiRect::left(Val::Px(10.0)),
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(3.0),
                    ..default()
                },
                BackgroundColor(if editing_key || editing_value {
                    ROW_BG_EDIT
                } else {
                    Color::NONE
                }),
            ))
            .with_children(|r| {
                r.spawn((
                    Text::new(format!("{shown_key}: {shown_value}")),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.72, 0.68, 0.62)),
                ));
            });
    }
    // "+ prop" button.
    parent.spawn((
        Button,
        EditorContentsAddPropButton { addr },
        Node {
            margin: UiRect::left(Val::Px(10.0)),
            padding: UiRect::horizontal(Val::Px(4.0)),
            ..default()
        },
        Text::new("+ prop"),
        TextFont {
            font_size: 10.0,
            ..default()
        },
        TextColor(Color::srgb(0.60, 0.56, 0.50)),
    ));
}

fn spawn_add_item_button(parent: &mut ChildSpawnerCommands, container: ContainerRef, full: bool) {
    let (label, color, border) = if full {
        (
            "(full)",
            Color::srgb(0.50, 0.46, 0.42),
            Color::srgb(0.22, 0.18, 0.14),
        )
    } else {
        (
            "+ Add item",
            Color::srgb(0.85, 0.80, 0.70),
            Color::srgb(0.30, 0.22, 0.14),
        )
    };
    let mut cmd = parent.spawn((
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.12, 0.09, 0.06, 0.80)),
        BorderColor::all(border),
    ));
    if !full {
        cmd.insert((Button, EditorContentsAddButton { container }));
    }
    cmd.with_children(|b| {
        b.spawn((
            Text::new(label),
            TextFont {
                font_size: 11.0,
                ..default()
            },
            TextColor(color),
        ));
    });
}

// ── Edit-focus clearing ──────────────────────────────────────────────────────

/// Drop any active edit in the property / stack / vendor panels so the
/// keyboard pipeline has a single owner when a contents cell takes focus.
fn steal_focus(
    prop_buffer: &mut EditorPropertyEditBuffer,
    stack_edit: &mut EditorStackEdit,
    vendor: &mut EditorVendorStashBuffer,
) {
    prop_buffer.editing_index = None;
    stack_edit.editing = false;
    stack_edit.text.clear();
    vendor.editing = None;
    vendor.edit_text.clear();
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// Reset the buffer's transient state whenever the selected object changes, so
/// a stale edit / pick arm never leaks onto a different (or despawned) object.
pub fn sync_contents_buffer_selection(
    editor_state: Res<EditorState>,
    mut buffer: ResMut<EditorContentsBuffer>,
) {
    if buffer.object_id != editor_state.selected_object_id {
        buffer.object_id = editor_state.selected_object_id;
        buffer.clear_transient();
    }
}

/// "+ Add item": arm a palette pick that fills the first empty slot of the
/// named container.
pub fn handle_contents_add_button(
    btns: Query<(&EditorContentsAddButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    editor_state: Res<EditorState>,
    mut buffer: ResMut<EditorContentsBuffer>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (btn, interaction) in &btns {
        if *interaction == Interaction::Pressed {
            buffer.editing = None;
            buffer.edit_text.clear();
            buffer.pending_item_pick = Some(ContentsPickTarget {
                object_id: selected,
                container: btn.container,
                replace: None,
            });
        }
    }
}

/// Type cell click: arm a palette pick that retypes that specific slot.
pub fn handle_contents_type_cell_click(
    cells: Query<(&EditorContentsTypeCell, &Interaction), (Changed<Interaction>, With<Button>)>,
    editor_state: Res<EditorState>,
    mut buffer: ResMut<EditorContentsBuffer>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (cell, interaction) in &cells {
        if *interaction == Interaction::Pressed {
            buffer.editing = None;
            buffer.edit_text.clear();
            buffer.pending_item_pick = Some(ContentsPickTarget {
                object_id: selected,
                container: cell.addr.container,
                replace: Some(cell.addr.index),
            });
        }
    }
}

/// Remove button: clear the slot then re-densify its container so no interior
/// gaps remain and `slots.len()` stays equal to capacity.
pub fn handle_contents_remove_button(
    btns: Query<(&EditorContentsRemoveButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut editor_state: ResMut<EditorState>,
    mut buffer: ResMut<EditorContentsBuffer>,
    mut containers: Query<(&OverworldObject, &mut Container)>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (btn, interaction) in &btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some((_, mut container)) = containers.iter_mut().find(|(o, _)| o.object_id == selected)
        else {
            continue;
        };
        let Some(slots) = container_slots_mut(&mut container, btn.addr.container) else {
            continue;
        };
        let capacity = slots.len();
        if btn.addr.index < slots.len() {
            slots[btn.addr.index] = None;
            slots.retain(|s| s.is_some());
            slots.resize(capacity, None);
            buffer.editing = None;
            buffer.edit_text.clear();
            buffer.set_changed();
            editor_state.dirty = true;
        }
    }
}

/// Toggle a top-level container item's nested-contents expansion.
pub fn handle_contents_expand_button(
    btns: Query<(&EditorContentsExpandButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut buffer: ResMut<EditorContentsBuffer>,
) {
    for (btn, interaction) in &btns {
        if *interaction == Interaction::Pressed {
            buffer.expanded = if buffer.expanded == Some(btn.top) {
                None
            } else {
                Some(btn.top)
            };
        }
    }
}

/// Quantity cell click: start editing the slot's stack count.
pub fn handle_contents_qty_cell_click(
    cells: Query<(&EditorContentsQtyCell, &Interaction), (Changed<Interaction>, With<Button>)>,
    editor_state: Res<EditorState>,
    mut buffer: ResMut<EditorContentsBuffer>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut stack_edit: ResMut<EditorStackEdit>,
    mut vendor: ResMut<EditorVendorStashBuffer>,
    containers: Query<(&OverworldObject, &Container)>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (cell, interaction) in &cells {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let current = containers
            .iter()
            .find(|(o, _)| o.object_id == selected)
            .and_then(|(_, c)| container_slots(c, cell.addr.container))
            .and_then(|slots| slots.get(cell.addr.index))
            .and_then(|s| s.as_ref())
            .map(|s| s.quantity)
            .unwrap_or(1);
        steal_focus(&mut prop_buffer, &mut stack_edit, &mut vendor);
        buffer.pending_item_pick = None;
        buffer.editing = Some(ContentsEditTarget::Quantity { addr: cell.addr });
        buffer.edit_text = current.to_string();
    }
}

/// Property row click: start editing that property's value.
pub fn handle_contents_prop_row_click(
    rows: Query<(&EditorContentsPropRow, &Interaction), (Changed<Interaction>, With<Button>)>,
    editor_state: Res<EditorState>,
    mut buffer: ResMut<EditorContentsBuffer>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut stack_edit: ResMut<EditorStackEdit>,
    mut vendor: ResMut<EditorVendorStashBuffer>,
    containers: Query<(&OverworldObject, &Container)>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (row, interaction) in &rows {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let value = containers
            .iter()
            .find(|(o, _)| o.object_id == selected)
            .and_then(|(_, c)| container_slots(c, row.addr.container))
            .and_then(|slots| slots.get(row.addr.index))
            .and_then(|s| s.as_ref())
            .and_then(|s| sorted_props(s).into_iter().nth(row.prop_index))
            .map(|(_, v)| v)
            .unwrap_or_default();
        steal_focus(&mut prop_buffer, &mut stack_edit, &mut vendor);
        buffer.pending_item_pick = None;
        buffer.editing = Some(ContentsEditTarget::PropertyValue {
            addr: row.addr,
            prop_index: row.prop_index,
        });
        buffer.edit_text = value;
    }
}

/// "+ prop": insert a blank property on the slot item and edit its key.
pub fn handle_contents_add_prop_button(
    btns: Query<(&EditorContentsAddPropButton, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut editor_state: ResMut<EditorState>,
    mut buffer: ResMut<EditorContentsBuffer>,
    mut prop_buffer: ResMut<EditorPropertyEditBuffer>,
    mut stack_edit: ResMut<EditorStackEdit>,
    mut vendor: ResMut<EditorVendorStashBuffer>,
    mut containers: Query<(&OverworldObject, &mut Container)>,
) {
    let Some(selected) = editor_state.selected_object_id else {
        return;
    };
    for (btn, interaction) in &btns {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some((_, mut container)) = containers.iter_mut().find(|(o, _)| o.object_id == selected)
        else {
            continue;
        };
        let Some(stack) = container_slots_mut(&mut container, btn.addr.container)
            .and_then(|slots| slots.get_mut(btn.addr.index))
            .and_then(|s| s.as_mut())
        else {
            continue;
        };
        // The new blank key sorts first (""), so its display index is 0.
        stack.properties.insert(String::new(), String::new());
        steal_focus(&mut prop_buffer, &mut stack_edit, &mut vendor);
        buffer.pending_item_pick = None;
        buffer.editing = Some(ContentsEditTarget::PropertyKey {
            addr: btn.addr,
            prop_index: 0,
        });
        buffer.edit_text.clear();
        buffer.set_changed();
        editor_state.dirty = true;
    }
}

/// Consume a palette click while a contents pick is armed: write the picked
/// item into the target container. Enforces capacity and the parent's
/// `accepts_storable_containers` rule. Ordered `.after(handle_palette_clicks)`
/// (which bails when a pick is armed) so the brush isn't also armed.
pub fn handle_contents_palette_pick(
    items: Query<(&EditorPaletteItem, &Interaction), (Changed<Interaction>, With<Button>)>,
    definitions: Res<OverworldObjectDefinitions>,
    mut buffer: ResMut<EditorContentsBuffer>,
    mut editor_state: ResMut<EditorState>,
    mut containers: Query<(&OverworldObject, &mut Container)>,
) {
    let Some(target) = buffer.pending_item_pick else {
        return;
    };
    for (item, interaction) in &items {
        if *interaction != Interaction::Pressed {
            continue;
        }
        buffer.pending_item_pick = None;

        // Parent container's type: the object itself (top-level) or the item
        // sitting in top slot `top` (nested).
        let Some((_, mut container)) = containers
            .iter_mut()
            .find(|(o, _)| o.object_id == target.object_id)
        else {
            break;
        };
        let parent_accepts = parent_accepts_containers(&container, target.container, &definitions);
        // Reject a storable container placed into a parent that forbids it
        // (keeps a pouch out of a pouch).
        if let Some(picked_def) = definitions.get(&item.type_id) {
            let picked_is_storable_container =
                picked_def.container_capacity.is_some() && picked_def.storable;
            if picked_is_storable_container && !parent_accepts {
                warn!(
                    "Contents: '{}' is a storable container and can't go inside this container",
                    item.type_id
                );
                break;
            }
        }

        let Some(slots) = container_slots_mut(&mut container, target.container) else {
            break;
        };
        let new_stack = InventoryStack::item(item.type_id.clone(), ObjectProperties::new(), 1);
        match target.replace {
            Some(index) if index < slots.len() => {
                slots[index] = Some(new_stack);
                editor_state.dirty = true;
            }
            Some(_) => {}
            None => {
                if let Some(empty) = slots.iter_mut().find(|s| s.is_none()) {
                    *empty = Some(new_stack);
                    editor_state.dirty = true;
                } else {
                    warn!("Contents: container is full");
                }
            }
        }
        // Force the properties panel to rebuild so the new item shows up.
        buffer.set_changed();
        break;
    }
}

impl InlineEditState for EditorContentsBuffer {
    fn is_editing(&self) -> bool {
        self.editing.is_some()
    }
    fn edit_text_mut(&mut self) -> &mut String {
        &mut self.edit_text
    }
    fn cancel_edit(&mut self) {
        self.editing = None;
        self.edit_text.clear();
    }
    fn has_pending_pick(&self) -> bool {
        self.pending_item_pick.is_some()
    }
    fn clear_pending_pick(&mut self) {
        self.pending_item_pick = None;
    }
}

/// Keyboard pipeline for the active contents cell. Shares the Esc / Enter /
/// Tab / Backspace / character loop (including Esc canceling an armed pick)
/// with the vendor-stash panel via `drive_inline_edit_keyboard`; only acts
/// when `buffer.editing` is `Some`. Commits on Enter / Tab.
pub fn handle_contents_keyboard_input(
    mut keyboard_events: bevy::ecs::message::MessageReader<bevy::input::keyboard::KeyboardInput>,
    definitions: Res<OverworldObjectDefinitions>,
    mut buffer: ResMut<EditorContentsBuffer>,
    mut editor_state: ResMut<EditorState>,
    mut containers: Query<(&OverworldObject, &mut Container)>,
) {
    // With an active edit but no selected object there is nothing to commit
    // into; leave the events unread (mirrors the pre-refactor early return).
    let selected = editor_state.selected_object_id;
    if buffer.editing.is_some() && selected.is_none() {
        return;
    }
    drive_inline_edit_keyboard(&mut keyboard_events, &mut *buffer, |buffer| {
        let Some(selected) = selected else {
            return;
        };
        commit_contents_edit(buffer, selected, &definitions, &mut containers);
        editor_state.dirty = true;
    });
}

fn commit_contents_edit(
    buffer: &mut EditorContentsBuffer,
    selected: u64,
    definitions: &OverworldObjectDefinitions,
    containers: &mut Query<(&OverworldObject, &mut Container)>,
) {
    let Some(editing) = buffer.editing.clone() else {
        return;
    };
    let text = std::mem::take(&mut buffer.edit_text);
    let Some((_, mut container)) = containers.iter_mut().find(|(o, _)| o.object_id == selected)
    else {
        buffer.editing = None;
        return;
    };
    match editing {
        ContentsEditTarget::Quantity { addr } => {
            if let Some(stack) = container_slots_mut(&mut container, addr.container)
                .and_then(|slots| slots.get_mut(addr.index))
                .and_then(|s| s.as_mut())
            {
                let max = definitions
                    .get(&stack.type_id)
                    .map(|d| d.max_stack_size)
                    .unwrap_or(1)
                    .max(1);
                let parsed = text.trim().parse::<u32>().unwrap_or(stack.quantity);
                stack.quantity = parsed.clamp(1, max);
            }
        }
        ContentsEditTarget::PropertyValue { addr, prop_index } => {
            if let Some(stack) = container_slots_mut(&mut container, addr.container)
                .and_then(|slots| slots.get_mut(addr.index))
                .and_then(|s| s.as_mut())
            {
                if let Some(key) = sorted_props(stack)
                    .into_iter()
                    .nth(prop_index)
                    .map(|(k, _)| k)
                {
                    stack.properties.insert(key, text);
                }
            }
        }
        ContentsEditTarget::PropertyKey { addr, prop_index } => {
            if let Some(stack) = container_slots_mut(&mut container, addr.container)
                .and_then(|slots| slots.get_mut(addr.index))
                .and_then(|s| s.as_mut())
            {
                if let Some((old_key, value)) = sorted_props(stack).into_iter().nth(prop_index) {
                    let new_key = text.trim().to_owned();
                    stack.properties.remove(&old_key);
                    if !new_key.is_empty() {
                        stack.properties.insert(new_key, value);
                    }
                }
            }
        }
    }
    buffer.editing = None;
}

// ── small utilities ──────────────────────────────────────────────────────────

fn parent_accepts_containers(
    container: &Container,
    cref: ContainerRef,
    definitions: &OverworldObjectDefinitions,
) -> bool {
    let type_id = match cref {
        None => return true, // top-level chest/barrel accept storables by default
        Some(top) => container
            .slots
            .get(top)
            .and_then(|s| s.as_ref())
            .map(|s| s.type_id.as_str()),
    };
    type_id
        .and_then(|t| definitions.get(t))
        .map(|d| d.accepts_storable_containers)
        .unwrap_or(true)
}
