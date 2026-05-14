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
use crate::ui::theme::{Palette, UiThemeAssets};

/// Stable identity for a movable window. Used to dedupe re-opens — if a
/// window with the same id already exists, callers should bring that one to
/// front instead of spawning a duplicate.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MovableWindowId {
    /// Item details popup keyed by the source slot. Re-Inspecting the same
    /// slot focuses the existing window instead of spawning another.
    ItemDetails(ItemSlotKind),
    /// The (singleton) active trade window. Lifecycle: spawned when
    /// `ClientGameState.current_trade` becomes `Some`, despawned when it
    /// returns to `None`.
    Trade,
    /// The (singleton) active NPC dialog window. Lifecycle: spawned when
    /// `ActiveDialogState.session_id` becomes `Some`, despawned when it
    /// returns to `None`.
    Dialog,
    /// Time-of-day detail popup. Toggled by the HUD time button; carries a
    /// clock readout, full-circle orbit visualization, and flavor text.
    TimeOfDay,
    /// Recipe book (singleton). Toggled by KeyC or opened with a station
    /// filter via right-click → "Craft" on a station object. Lists every
    /// learned recipe with input availability indicators and a Craft
    /// button per row.
    RecipeBook,
}

#[derive(Component)]
pub struct MovableWindow {
    pub id: MovableWindowId,
    /// Lower bound enforced by the shared bottom-right resize handle.
    pub min_size: Vec2,
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

/// Marker on the small bottom-right grab patch that resizes the window.
/// Auto-spawned by [`spawn_movable_window`] so every popup gets resize for
/// free; `owner` points at the window root so the resize system can update
/// its `Node` size without walking the hierarchy.
#[derive(Component)]
pub struct MovableWindowResizeHandle {
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

#[derive(Resource, Default)]
pub struct MovableWindowResize {
    /// Window currently being resized via its bottom-right handle.
    pub active: Option<Entity>,
}

pub const MOVABLE_WINDOW_Z_BASE: i32 = i32::MAX - 20;
pub const MOVABLE_WINDOW_Z_FOCUSED: i32 = i32::MAX - 11;
pub const MOVABLE_WINDOW_CASCADE_PX: f32 = 32.0;
pub const MOVABLE_WINDOW_RESIZE_HANDLE_PX: f32 = 18.0;
pub const MOVABLE_WINDOW_DEFAULT_MIN_SIZE: Vec2 = Vec2::new(220.0, 160.0);

pub struct MovableWindowPlugin;

impl Plugin for MovableWindowPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(MovableWindowDrag::default())
            .insert_resource(MovableWindowResize::default())
            .add_systems(
                Update,
                (
                    handle_movable_window_resize,
                    handle_movable_window_drag.after(handle_movable_window_resize),
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

/// Entity handles returned by [`spawn_movable_window`]. The consumer adds
/// their content as children of `body` and (optionally) a custom close
/// button or other widgets as children of `title_bar`.
pub struct MovableWindowEntities {
    pub root: Entity,
    pub body: Entity,
    pub title_bar: Entity,
}

/// Spawn a bare movable window: root with the panel-frame background, a
/// draggable title bar with the title text, an empty body, and an
/// auto-spawned resize handle in the bottom-right corner. Does **not**
/// spawn a close button — call [`spawn_movable_window_close_button`] from
/// the consumer if the standard close-X is desired, or build a custom one
/// (e.g. trade's CancelTrade-emitting button).
///
/// `min_size` is the floor enforced by the shared resize system; pass
/// [`MOVABLE_WINDOW_DEFAULT_MIN_SIZE`] if no per-window minimum is needed.
#[allow(clippy::too_many_arguments)]
pub fn spawn_movable_window(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    id: MovableWindowId,
    title: &str,
    size: Vec2,
    initial_pos: Vec2,
    min_size: Vec2,
) -> MovableWindowEntities {
    let clamped_size = size.max(min_size);
    let root = commands
        .spawn((
            MovableWindow { id, min_size },
            Node {
                position_type: PositionType::Absolute,
                left: px(initial_pos.x),
                top: px(initial_pos.y),
                width: px(clamped_size.x.max(1.0)),
                height: px(clamped_size.y.max(1.0)),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            ImageNode::new(theme.panel_frame.clone())
                .with_mode(theme.panel_image_mode())
                .with_color(Color::WHITE),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_accent),
            GlobalZIndex(MOVABLE_WINDOW_Z_BASE),
        ))
        .id();

    let title_bar = commands
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
            ImageNode::new(theme.title_bar.clone())
                .with_mode(theme.title_bar_image_mode())
                .with_color(Color::WHITE),
            BackgroundColor(Color::NONE),
            BorderColor::all(palette.border_slot),
        ))
        .id();

    commands.entity(title_bar).with_children(|bar| {
        bar.spawn((
            Text::new(title.to_owned()),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(palette.text_accent),
        ));
    });

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

    let resize_handle = commands
        .spawn((
            MovableWindowResizeHandle { owner: root },
            Node {
                position_type: PositionType::Absolute,
                right: px(0.0),
                bottom: px(0.0),
                width: px(MOVABLE_WINDOW_RESIZE_HANDLE_PX),
                height: px(MOVABLE_WINDOW_RESIZE_HANDLE_PX),
                ..default()
            },
            ImageNode::new(theme.resize_corner.clone()),
        ))
        .id();

    commands
        .entity(root)
        .add_children(&[title_bar, body, resize_handle]);

    MovableWindowEntities {
        root,
        body,
        title_bar,
    }
}

/// Shared visual for any "X" close button in a movable window — the brass
/// medallion (no rectangular frame underneath). `marker` distinguishes
/// which close-handler picks up the click (the standard despawn handler
/// vs. Dialog's `DialogEnd` emission vs. Trade's `CancelTrade`).
pub fn spawn_themed_close_button<M: Bundle>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    marker: M,
) {
    parent.spawn((
        Button,
        marker,
        Node {
            width: px(18.0),
            height: px(18.0),
            ..default()
        },
        ImageNode::new(theme.close_button.clone()),
        BackgroundColor(Color::NONE),
    ));
}

/// Spawn the default close button as a child of `parent` (typically the
/// title bar). Click despawns `owner` via [`handle_movable_window_close`].
/// The `palette` arg is unused but kept in the signature so existing call
/// sites stay unchanged.
pub fn spawn_movable_window_close_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    _palette: &Palette,
    owner: Entity,
) {
    spawn_themed_close_button(parent, theme, MovableWindowCloseButton { owner });
}

fn handle_movable_window_resize(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut resize: ResMut<MovableWindowResize>,
    mut drag: ResMut<MovableWindowDrag>,
    handle_query: Query<(
        &MovableWindowResizeHandle,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    movable_query: Query<&MovableWindow>,
    mut node_query: Query<&mut Node, With<MovableWindow>>,
) {
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        if resize.active.is_some() {
            resize.active = None;
        }
        return;
    };

    if !mouse_input.pressed(MouseButton::Left) {
        if resize.active.is_some() {
            resize.active = None;
        }
        return;
    }

    if mouse_input.just_pressed(MouseButton::Left) && resize.active.is_none() {
        for (handle, computed, transform) in &handle_query {
            if point_in_node(cursor, computed, transform) {
                resize.active = Some(handle.owner);
                drag.focused = Some(handle.owner);
                break;
            }
        }
    }

    if let Some(entity) = resize.active {
        let Ok(window_marker) = movable_query.get(entity) else {
            resize.active = None;
            return;
        };
        let min_size = window_marker.min_size;
        if let Ok(mut node) = node_query.get_mut(entity) {
            let top_left = Vec2::new(val_to_px(node.left), val_to_px(node.top));
            let new_size = (cursor - top_left).max(min_size).max(Vec2::splat(1.0));
            let target_w = px(new_size.x);
            let target_h = px(new_size.y);
            if node.width != target_w {
                node.width = target_w;
            }
            if node.height != target_h {
                node.height = target_h;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_movable_window_drag(
    mouse_input: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    resize: Res<MovableWindowResize>,
    mut drag: ResMut<MovableWindowDrag>,
    title_query: Query<(&MovableWindowTitleBar, &ComputedNode, &UiGlobalTransform)>,
    button_query: Query<(&ComputedNode, &UiGlobalTransform, Option<&Visibility>), With<Button>>,
    handle_query: Query<(&ComputedNode, &UiGlobalTransform), With<MovableWindowResizeHandle>>,
    window_box_query: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<MovableWindow>>,
    mut node_query: Query<&mut Node, With<MovableWindow>>,
) {
    if resize.active.is_some() {
        // A resize is in progress; never start a drag while the user is
        // pulling a handle.
        if drag.dragging.is_some() {
            drag.dragging = None;
        }
        return;
    }

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
        // Mouse-down on a resize handle is owned by the resize system —
        // don't also focus / drag-start from it.
        let on_resize_handle = handle_query
            .iter()
            .any(|(node, transform)| point_in_node(cursor, node, transform));
        if on_resize_handle {
            return;
        }
        // Any visible button under the cursor short-circuits drag-start so
        // that close buttons, footer actions, etc. always win over the
        // title-bar grab. The button itself handles the press via its own
        // Interaction-driven system.
        let on_button = button_query.iter().any(|(node, transform, visibility)| {
            visibility.is_none_or(|v| *v != Visibility::Hidden)
                && point_in_node(cursor, node, transform)
        });
        if !on_button {
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
        }
        // Body click (including clicks on buttons inside a window): pick the
        // topmost window the cursor is over and focus it. Last match wins —
        // for a small set of windows that's an acceptable tie-breaker.
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
    // `point` is in logical pixels (from `Window::cursor_position()`), while
    // `ComputedNode` / `UiGlobalTransform` are in physical pixels. Scale up
    // before hit-testing so this works on HiDPI displays.
    let inv = computed.inverse_scale_factor();
    let physical_point = if inv > 0.0 { point / inv } else { point };
    computed.contains_point(*transform, physical_point)
}

pub(crate) fn val_to_px(val: Val) -> f32 {
    match val {
        Val::Px(v) => v,
        _ => 0.0,
    }
}
