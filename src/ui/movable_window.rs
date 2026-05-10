//! Reusable floating-window primitive.
//!
//! A "movable window" is a top-level UI node with a title bar (drag handle +
//! close X) and an exposed body. Multiple windows can coexist; left-click on
//! any window's title bar drags it, left-click anywhere inside a window
//! brings it to front, and the close button despawns it.
//!
//! Consumers spawn one via [`spawn_movable_window`] (which returns the root
//! entity and the body entity for further child population) and attach their
//! own marker on the body so a per-frame sync system can rebuild its
//! contents.

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::app::state::ClientAppState;
use crate::ui::components::ItemSlotKind;
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton};
use crate::ui::theme::{Palette, UiThemeAssets};

/// Stable identity for a movable window. Used to dedupe re-opens — if a
/// window with the same id already exists, callers should bring that one to
/// front instead of spawning a duplicate.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MovableWindowId {
    /// Item details popup keyed by the source slot. Re-Inspecting the same
    /// slot focuses the existing window instead of spawning another.
    ItemDetails(ItemSlotKind),
}

#[derive(Component)]
pub struct MovableWindow {
    pub id: MovableWindowId,
}

/// Marker on the title-bar node inside a window. `owner` points at the
/// window's root entity so the drag system can update its `Node` position
/// without walking the hierarchy.
#[derive(Component)]
pub struct MovableWindowTitleBar {
    pub owner: Entity,
}

#[derive(Component)]
pub struct MovableWindowCloseButton {
    pub owner: Entity,
}

/// Marker on the body node inside a window. Consumers attach their own
/// `*Content` component alongside this and rebuild children of this node when
/// the underlying data changes.
#[derive(Component)]
pub struct MovableWindowContent {
    pub owner: Entity,
}

#[derive(Resource, Default)]
pub struct MovableWindowDrag {
    /// `(window root entity, cursor → window-top-left offset)`. `None` when
    /// no window is being dragged.
    pub dragging: Option<(Entity, Vec2)>,
    /// Window currently on top — receives a z-index boost.
    pub focused: Option<Entity>,
}

pub const MOVABLE_WINDOW_Z_BASE: i32 = i32::MAX - 20;
pub const MOVABLE_WINDOW_Z_FOCUSED: i32 = i32::MAX - 11;
pub const MOVABLE_WINDOW_CASCADE_PX: f32 = 32.0;

pub struct MovableWindowPlugin;

impl Plugin for MovableWindowPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MovableWindowDrag::default())
            .add_systems(
                Update,
                (
                    handle_movable_window_drag,
                    handle_movable_window_close,
                    apply_movable_window_focus_z_index.after(handle_movable_window_drag),
                )
                    .run_if(in_state(ClientAppState::InGame)),
            );
    }
}

/// Find the entity of an existing window with `id`, if any. Use this from
/// "open-this-window" callers to dedupe re-opens.
pub fn find_window_by_id(
    query: &Query<(Entity, &MovableWindow)>,
    id: MovableWindowId,
) -> Option<Entity> {
    query
        .iter()
        .find(|(_, window)| window.id == id)
        .map(|(entity, _)| entity)
}

/// Spawn a movable window. Returns `(root_entity, body_entity)` so the
/// consumer can attach a content-marker to the body and populate children:
///
/// ```ignore
/// let (root, body) = spawn_movable_window(&mut commands, &theme, &palette, id, "Title",
///                                          Vec2::new(360.0, 420.0), pos);
/// commands.entity(body)
///     .insert(MyContentMarker { ... })
///     .with_children(|parent| { ... });
/// ```
pub fn spawn_movable_window(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    id: MovableWindowId,
    title: &str,
    size: Vec2,
    initial_pos: Vec2,
) -> (Entity, Entity) {
    let root = commands
        .spawn((
            MovableWindow { id },
            Node {
                position_type: PositionType::Absolute,
                left: px(initial_pos.x),
                top: px(initial_pos.y),
                width: px(size.x.max(1.0)),
                height: px(size.y.max(1.0)),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            ImageNode::new(theme.panel_frame.clone())
                .with_mode(theme.panel_image_mode())
                .with_color(palette.surface_panel),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_accent),
            GlobalZIndex(MOVABLE_WINDOW_Z_BASE),
        ))
        .id();

    let body = commands
        .spawn((
            MovableWindowContent { owner: root },
            Node {
                width: percent(100.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(6.0),
                padding: UiRect::all(px(10.0)),
                min_height: px(0.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .id();

    commands.entity(root).with_children(|parent| {
        parent
            .spawn((
                MovableWindowTitleBar { owner: root },
                Node {
                    width: percent(100.0),
                    height: px(26.0),
                    padding: UiRect::axes(px(8.0), px(2.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    border: UiRect::bottom(px(1.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(palette.surface_raised),
                BorderColor::all(palette.border_slot),
            ))
            .with_children(|bar| {
                bar.spawn((
                    Text::new(title.to_owned()),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(palette.text_accent),
                ));
                spawn_close_button(bar, theme, palette, root);
            });
    });
    commands.entity(root).add_child(body);

    (root, body)
}

fn spawn_close_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    owner: Entity,
) {
    let (bg, border, _text) = idle_colors(palette, ButtonStyle::Secondary, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Secondary),
            MovableWindowCloseButton { owner },
            Node {
                width: px(22.0),
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
                Text::new("X"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(palette.text_primary),
            ));
        });
}

fn handle_movable_window_drag(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut drag: ResMut<MovableWindowDrag>,
    title_query: Query<(&MovableWindowTitleBar, &ComputedNode, &UiGlobalTransform)>,
    close_query: Query<(&MovableWindowCloseButton, &ComputedNode, &UiGlobalTransform)>,
    window_box_query: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<MovableWindow>>,
    mut node_query: Query<&mut Node, With<MovableWindow>>,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        if drag.dragging.is_some() {
            drag.dragging = None;
        }
        return;
    };

    if !mouse_input.pressed(MouseButton::Left) {
        if drag.dragging.is_some() {
            drag.dragging = None;
        }
        return;
    }

    if mouse_input.just_pressed(MouseButton::Left) {
        // Close button: focus the owning window but never start a drag.
        for (close, computed, transform) in &close_query {
            if point_in_node(cursor, computed, transform) {
                drag.focused = Some(close.owner);
                return;
            }
        }
        // Title bar: focus + start drag, anchoring at current top-left.
        for (bar, computed, transform) in &title_query {
            if point_in_node(cursor, computed, transform) {
                let position = match node_query.get(bar.owner) {
                    Ok(node) => Vec2::new(val_to_px(node.left), val_to_px(node.top)),
                    Err(_) => continue,
                };
                drag.dragging = Some((bar.owner, cursor - position));
                drag.focused = Some(bar.owner);
                return;
            }
        }
        // Body click: pick the topmost window the cursor is over and focus it.
        // No fancy z-stack — the last match wins because UI iteration order
        // is stable per archetype; any tie-breaker is fine for a small set.
        let mut best: Option<Entity> = None;
        for (entity, computed, transform) in &window_box_query {
            if point_in_node(cursor, computed, transform) {
                best = Some(entity);
            }
        }
        if let Some(entity) = best {
            drag.focused = Some(entity);
        }
    }

    if let Some((entity, offset)) = drag.dragging {
        if let Ok(mut node) = node_query.get_mut(entity) {
            let new_pos = cursor - offset;
            let target_left = px(new_pos.x);
            let target_top = px(new_pos.y);
            if node.left != target_left {
                node.left = target_left;
            }
            if node.top != target_top {
                node.top = target_top;
            }
        }
    }
}

fn handle_movable_window_close(
    mut commands: Commands,
    mut drag: ResMut<MovableWindowDrag>,
    interactions: Query<(&Interaction, &MovableWindowCloseButton), Changed<Interaction>>,
    window_query: Query<Entity, With<MovableWindow>>,
) {
    for (interaction, close) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if window_query.get(close.owner).is_err() {
            continue;
        }
        commands.entity(close.owner).despawn();
        if drag.focused == Some(close.owner) {
            drag.focused = None;
        }
        if drag.dragging.is_some_and(|(e, _)| e == close.owner) {
            drag.dragging = None;
        }
    }
}

fn apply_movable_window_focus_z_index(
    drag: Res<MovableWindowDrag>,
    mut query: Query<(Entity, &mut GlobalZIndex), With<MovableWindow>>,
) {
    if !drag.is_changed() {
        return;
    }
    let focused = drag.focused;
    for (entity, mut z) in &mut query {
        let target = if Some(entity) == focused {
            GlobalZIndex(MOVABLE_WINDOW_Z_FOCUSED)
        } else {
            GlobalZIndex(MOVABLE_WINDOW_Z_BASE)
        };
        if *z != target {
            *z = target;
        }
    }
}

fn point_in_node(point: Vec2, computed: &ComputedNode, transform: &UiGlobalTransform) -> bool {
    computed.contains_point(*transform, point)
}

fn val_to_px(val: Val) -> f32 {
    match val {
        Val::Px(v) => v,
        _ => 0.0,
    }
}
