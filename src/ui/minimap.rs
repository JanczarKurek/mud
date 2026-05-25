use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::UiGlobalTransform;

use crate::game::resources::{ClientGameState, ClientWorldObjectState};
use crate::ui::components::{
    FloatingMinimapZoomInButton, FloatingMinimapZoomLabel, FloatingMinimapZoomOutButton,
    HudMinimapZoomInButton, HudMinimapZoomLabel, HudMinimapZoomOutButton, MinimapCanvas,
    MinimapMode, MinimapOverlayDot, MinimapView,
};
use crate::ui::mountable_panel::PanelMountMode;
use crate::ui::resources::{
    DockedPanelState, FloatingMinimapPan, FloatingMinimapPanDrag, FloatingMinimapZoom,
    HudMinimapSettings, MinimapPanelMode, MinimapZoom,
};
use crate::world::components::SpaceId;
use crate::world::floor_definitions::FloorTilesetDefinitions;
use crate::world::object_definitions::OverworldObjectDefinitions;

/// Pixel size of the HUD minimap UI node (square).
pub const HUD_MINIMAP_SIZE: f32 = 220.0;
/// Pixel size of the full-map window's rendered minimap body (square).
pub const FULL_MAP_BODY_SIZE: f32 = 520.0;

const OUT_OF_BOUNDS_COLOR: [u8; 4] = [12, 10, 14, 255];
const DEFAULT_FILL_COLOR: [u8; 4] = [40, 56, 40, 255];

pub fn make_minimap_image(zoom: MinimapZoom) -> Image {
    let span = zoom.tile_span() as u32;
    let mut image = Image::new_fill(
        Extent3d {
            width: span,
            height: span,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &OUT_OF_BOUNDS_COLOR,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::all(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
    image
}

/// Repaints the Image backing each MinimapView, and rebuilds overlay dot
/// children. Runs every frame; cost is ~O(zoom_span^2) bytes written plus a
/// small dot respawn — cheap even at `Far`.
pub fn update_minimap_images(
    mut commands: Commands,
    client_state: Res<ClientGameState>,
    object_definitions: Res<OverworldObjectDefinitions>,
    floor_definitions: Res<FloorTilesetDefinitions>,
    hud_settings: Res<HudMinimapSettings>,
    floating_zoom: Res<FloatingMinimapZoom>,
    floating_pan: Res<FloatingMinimapPan>,
    mut images: ResMut<Assets<Image>>,
    mut views: Query<(
        Entity,
        &MinimapView,
        &mut MinimapCanvas,
        &mut ImageNode,
        &ComputedNode,
        Option<&Children>,
    )>,
    overlay_dots: Query<Entity, With<MinimapOverlayDot>>,
) {
    let player_space = client_state
        .current_space
        .as_ref()
        .map(|space| space.space_id);
    let player_tile = client_state.player_tile_position;
    let fill_color = fill_color_rgba(&client_state, &floor_definitions);

    let pan_offset = IVec2::new(
        floating_pan.offset_tiles.x.round() as i32,
        floating_pan.offset_tiles.y.round() as i32,
    );

    for (entity, view, mut canvas, mut image_node, computed, children) in views.iter_mut() {
        let zoom = match view.mode {
            MinimapMode::HudSmall => hud_settings.zoom,
            MinimapMode::FullscreenLarge => floating_zoom.0,
        };
        let view_pan = match view.mode {
            MinimapMode::HudSmall => IVec2::ZERO,
            MinimapMode::FullscreenLarge => pan_offset,
        };

        let zoom_changed = canvas.last_zoom != Some(zoom);
        if zoom_changed {
            let new_image = make_minimap_image(zoom);
            let new_handle = images.add(new_image);
            canvas.image_handle = new_handle.clone();
            canvas.last_zoom = Some(zoom);
            image_node.image = new_handle;
        }

        let span = zoom.tile_span();

        if let (Some(space_id), Some(tile)) = (player_space, player_tile) {
            let center_x = tile.x + view_pan.x;
            let center_y = tile.y + view_pan.y;
            if let Some(image) = images.get_mut(&canvas.image_handle) {
                paint_tile_window(
                    image,
                    span,
                    space_id,
                    center_x,
                    center_y,
                    tile.z,
                    fill_color,
                    &client_state,
                    &object_definitions,
                );
            }
        } else if let Some(image) = images.get_mut(&canvas.image_handle) {
            fill_image(image, OUT_OF_BOUNDS_COLOR);
        }

        if let Some(children) = children {
            for child in children.iter() {
                if overlay_dots.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
        }

        if let (Some(space_id), Some(tile)) = (player_space, player_tile) {
            let half_span = (span - 1) / 2;
            let node_size = computed.size();
            let fallback_size = match view.mode {
                MinimapMode::HudSmall => HUD_MINIMAP_SIZE,
                MinimapMode::FullscreenLarge => FULL_MAP_BODY_SIZE,
            };
            // ComputedNode may be zero on the first frame before layout
            // resolves; fall back to the authored constant so dots appear
            // somewhere reasonable until the next tick.
            let view_width = if node_size.x > 0.0 {
                node_size.x
            } else {
                fallback_size
            };
            let view_height = if node_size.y > 0.0 {
                node_size.y
            } else {
                fallback_size
            };
            let tile_ui_x = view_width / span as f32;
            let tile_ui_y = view_height / span as f32;
            let tile_ui = tile_ui_x.min(tile_ui_y);

            let player_dot_size = tile_ui.min(12.0).max(2.0);
            let other_dot_size = (tile_ui * 0.75).min(10.0).max(2.0);

            let center_x = tile.x + view_pan.x;
            let center_y = tile.y + view_pan.y;

            commands.entity(entity).with_children(|dots| {
                // Player position relative to view center (zero when not
                // panned). Skipped when the player has been panned out of
                // the visible window.
                let player_dx = tile.x - center_x;
                let player_dy = tile.y - center_y;
                if player_dx.abs() <= half_span && player_dy.abs() <= half_span {
                    spawn_dot(
                        dots,
                        tile_ui_x,
                        tile_ui_y,
                        half_span,
                        player_dx,
                        player_dy,
                        Color::srgb(1.0, 1.0, 1.0),
                        player_dot_size,
                    );
                }

                for remote in client_state.remote_players.values() {
                    if remote.position.space_id != space_id {
                        continue;
                    }
                    if remote.tile_position.z != tile.z {
                        continue;
                    }
                    let dx = remote.tile_position.x - center_x;
                    let dy = remote.tile_position.y - center_y;
                    if dx.abs() > half_span || dy.abs() > half_span {
                        continue;
                    }
                    spawn_dot(
                        dots,
                        tile_ui_x,
                        tile_ui_y,
                        half_span,
                        dx,
                        dy,
                        Color::srgb(0.45, 0.70, 1.0),
                        other_dot_size,
                    );
                }

                for object in client_state.world_objects.values() {
                    if object.position.space_id != space_id {
                        continue;
                    }
                    if object.tile_position.z != tile.z {
                        continue;
                    }
                    if !object.is_npc && !object.is_container {
                        continue;
                    }
                    let dx = object.tile_position.x - center_x;
                    let dy = object.tile_position.y - center_y;
                    if dx.abs() > half_span || dy.abs() > half_span {
                        continue;
                    }
                    let color = if object.is_npc {
                        Color::srgb(0.95, 0.32, 0.30)
                    } else {
                        Color::srgb(0.95, 0.80, 0.30)
                    };
                    spawn_dot(
                        dots,
                        tile_ui_x,
                        tile_ui_y,
                        half_span,
                        dx,
                        dy,
                        color,
                        other_dot_size,
                    );
                }
            });
        }
    }
}

fn fill_color_rgba(
    client_state: &ClientGameState,
    definitions: &FloorTilesetDefinitions,
) -> [u8; 4] {
    let Some(space) = client_state.current_space.as_ref() else {
        return DEFAULT_FILL_COLOR;
    };
    let Some(definition) = definitions.get(&space.fill_floor_type) else {
        return DEFAULT_FILL_COLOR;
    };
    let [r, g, b] = definition.debug_color;
    [r, g, b, 255]
}

fn tile_color_rgba(
    object: &ClientWorldObjectState,
    definitions: &OverworldObjectDefinitions,
) -> Option<[u8; 4]> {
    let definition = definitions.get(&object.definition_id)?;
    let [r, g, b] = definition.render.debug_color;
    Some([r, g, b, 255])
}

fn paint_tile_window(
    image: &mut Image,
    span: i32,
    space_id: SpaceId,
    player_x: i32,
    player_y: i32,
    player_z: i32,
    fill: [u8; 4],
    client_state: &ClientGameState,
    definitions: &OverworldObjectDefinitions,
) {
    let Some(data) = image.data.as_mut() else {
        return;
    };
    let bpp = 4;
    let span_usize = span as usize;

    if data.len() != span_usize * span_usize * bpp {
        return;
    }

    let space_width = client_state
        .current_space
        .as_ref()
        .map(|space| space.width)
        .unwrap_or(0);
    let space_height = client_state
        .current_space
        .as_ref()
        .map(|space| space.height)
        .unwrap_or(0);

    let discovered = client_state.discovered_tiles.get(&space_id);

    let half = (span - 1) / 2;

    for row in 0..span_usize {
        // Row 0 is the top of the displayed image; game convention puts north
        // (higher y) at the top of the screen, so the top row must show
        // `player_y + half` and the bottom row `player_y - half`.
        let world_y = player_y + half - row as i32;
        for col in 0..span_usize {
            let world_x = player_x - half + col as i32;
            let color = if world_x < 0
                || world_y < 0
                || world_x >= space_width
                || world_y >= space_height
            {
                OUT_OF_BOUNDS_COLOR
            } else if discovered.is_some_and(|set| set.contains(&(world_x, world_y))) {
                fill
            } else {
                fog_color_for_tile(world_x, world_y)
            };
            let pixel_idx = (row * span_usize + col) * bpp;
            data[pixel_idx] = color[0];
            data[pixel_idx + 1] = color[1];
            data[pixel_idx + 2] = color[2];
            data[pixel_idx + 3] = color[3];
        }
    }

    for object in client_state.world_objects.values() {
        if object.position.space_id != space_id {
            continue;
        }
        if object.tile_position.z != player_z {
            continue;
        }
        if object.is_npc || object.is_movable || object.is_container {
            continue;
        }
        // Don't reveal object positions on tiles the player hasn't seen.
        if !discovered
            .is_some_and(|set| set.contains(&(object.tile_position.x, object.tile_position.y)))
        {
            continue;
        }
        let dx = object.tile_position.x - player_x;
        let dy = object.tile_position.y - player_y;
        if dx.abs() > half || dy.abs() > half {
            continue;
        }
        let col = (dx + half) as usize;
        let row = (half - dy) as usize;
        let Some(color) = tile_color_rgba(object, definitions) else {
            continue;
        };
        let pixel_idx = (row * span_usize + col) * bpp;
        data[pixel_idx] = color[0];
        data[pixel_idx + 1] = color[1];
        data[pixel_idx + 2] = color[2];
        data[pixel_idx + 3] = color[3];
    }
}

/// Deterministic star pattern for undiscovered minimap cells. ~1/64 cells
/// gets a bright pixel, the rest are deep-indigo background — same visual
/// language as the main-view fog shader so the two views read as the same
/// "unexplored" state.
fn fog_color_for_tile(x: i32, y: i32) -> [u8; 4] {
    let mut h: u32 = 0x9e3779b9;
    h ^= (x as u32).wrapping_mul(0x85ebca6b);
    h = h.rotate_left(13).wrapping_mul(0xc2b2ae35);
    h ^= (y as u32).wrapping_mul(0x27d4eb2f);
    h ^= h >> 16;
    match h & 0x3F {
        0 => [220, 220, 240, 255],
        1..=2 => [120, 120, 150, 255],
        _ => [8, 6, 14, 255],
    }
}

fn fill_image(image: &mut Image, color: [u8; 4]) {
    let Some(data) = image.data.as_mut() else {
        return;
    };
    for chunk in data.chunks_exact_mut(4) {
        chunk.copy_from_slice(&color);
    }
}

fn spawn_dot(
    dots: &mut ChildSpawnerCommands,
    tile_ui_x: f32,
    tile_ui_y: f32,
    half_span: i32,
    dx: i32,
    dy: i32,
    color: Color,
    size: f32,
) {
    let center_x = (half_span as f32 + 0.5) * tile_ui_x;
    let center_y = (half_span as f32 + 0.5) * tile_ui_y;
    // Mirror Y so north (+y in game) draws above the player on the minimap,
    // matching the main view. X is already consistent (east = right).
    let cx = center_x + dx as f32 * tile_ui_x;
    let cy = center_y - dy as f32 * tile_ui_y;
    dots.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(cx - size * 0.5),
            top: Val::Px(cy - size * 0.5),
            width: Val::Px(size),
            height: Val::Px(size),
            ..default()
        },
        BackgroundColor(color),
        MinimapOverlayDot,
    ));
}

pub fn sync_minimap_zoom_labels(
    hud_settings: Res<HudMinimapSettings>,
    floating_zoom: Res<FloatingMinimapZoom>,
    mut hud_labels: Query<
        &mut Text,
        (With<HudMinimapZoomLabel>, Without<FloatingMinimapZoomLabel>),
    >,
    mut floating_labels: Query<
        &mut Text,
        (With<FloatingMinimapZoomLabel>, Without<HudMinimapZoomLabel>),
    >,
) {
    for mut text in &mut hud_labels {
        text.0 = hud_settings.zoom.label().to_owned();
    }
    for mut text in &mut floating_labels {
        text.0 = floating_zoom.0.label().to_owned();
    }
}

pub fn handle_minimap_zoom_buttons(
    mut hud_settings: ResMut<HudMinimapSettings>,
    mut floating_zoom: ResMut<FloatingMinimapZoom>,
    hud_in: Query<&Interaction, (Changed<Interaction>, With<HudMinimapZoomInButton>)>,
    hud_out: Query<&Interaction, (Changed<Interaction>, With<HudMinimapZoomOutButton>)>,
    floating_in: Query<&Interaction, (Changed<Interaction>, With<FloatingMinimapZoomInButton>)>,
    floating_out: Query<&Interaction, (Changed<Interaction>, With<FloatingMinimapZoomOutButton>)>,
) {
    for interaction in &hud_in {
        if *interaction == Interaction::Pressed {
            hud_settings.zoom = hud_settings.zoom.zoom_in();
        }
    }
    for interaction in &hud_out {
        if *interaction == Interaction::Pressed {
            hud_settings.zoom = hud_settings.zoom.zoom_out();
        }
    }
    for interaction in &floating_in {
        if *interaction == Interaction::Pressed {
            floating_zoom.0 = floating_zoom.0.zoom_in();
        }
    }
    for interaction in &floating_out {
        if *interaction == Interaction::Pressed {
            floating_zoom.0 = floating_zoom.0.zoom_out();
        }
    }
}

/// `M` toggles the minimap panel between docked and floating; `+`/`-`
/// (`FullMapZoomIn/Out`) adjust the floating zoom while floating.
pub fn handle_minimap_keybinds(
    keys: Res<ButtonInput<KeyCode>>,
    keybindings: Res<crate::ui::settings::Keybindings>,
    mut panel_state: ResMut<DockedPanelState>,
    mut modes: ResMut<MinimapPanelMode>,
    mut floating_zoom: ResMut<FloatingMinimapZoom>,
) {
    use crate::ui::minimap_panel::MinimapPanel;
    use crate::ui::mountable_panel::MountablePanel;
    use crate::ui::settings::model::Action;
    if keybindings.just_pressed(Action::ToggleFullMap, &keys) {
        // Ensure the panel is registered in the dock so toggling has
        // something to act on. If the player previously closed it, push
        // its docked definition back in.
        let panel_id = MinimapPanel::panel_id_for(());
        if !panel_state.is_open(panel_id) {
            if let Some(def) = MinimapPanel::docked_definition(()) {
                panel_state.panels.push(def);
            }
        }
        modes.0 = match modes.0 {
            PanelMountMode::Mounted => PanelMountMode::Floating {
                last_position: MinimapPanel::floating_position(()),
            },
            PanelMountMode::Floating { .. } => PanelMountMode::Mounted,
        };
    }
    if matches!(modes.0, PanelMountMode::Floating { .. }) {
        if keybindings.just_pressed(Action::FullMapZoomIn, &keys) {
            floating_zoom.0 = floating_zoom.0.zoom_in();
        }
        if keybindings.just_pressed(Action::FullMapZoomOut, &keys) {
            floating_zoom.0 = floating_zoom.0.zoom_out();
        }
    }
}

pub fn handle_minimap_scroll_wheel(
    mut scroll_events: MessageReader<MouseWheel>,
    mut hud_settings: ResMut<HudMinimapSettings>,
    mut floating_zoom: ResMut<FloatingMinimapZoom>,
    windows: Query<&Window>,
    hud_views: Query<(&MinimapView, &ComputedNode, &UiGlobalTransform)>,
) {
    let Some(cursor) = windows
        .iter()
        .next()
        .and_then(|window| window.cursor_position())
    else {
        scroll_events.clear();
        return;
    };

    let mut scroll_total = 0.0_f32;
    for event in scroll_events.read() {
        scroll_total += event.y;
    }
    if scroll_total == 0.0 {
        return;
    }

    for (view, computed, transform) in hud_views.iter() {
        let size = computed.size();
        if size.x <= 0.0 || size.y <= 0.0 {
            continue;
        }
        let center = transform.translation;
        let half = size * 0.5;
        let min = center - half;
        let max = center + half;
        if cursor.x < min.x || cursor.x > max.x || cursor.y < min.y || cursor.y > max.y {
            continue;
        }
        let state_zoom = match view.mode {
            MinimapMode::HudSmall => &mut hud_settings.zoom,
            MinimapMode::FullscreenLarge => &mut floating_zoom.0,
        };
        if scroll_total > 0.0 {
            *state_zoom = state_zoom.zoom_in();
        } else {
            *state_zoom = state_zoom.zoom_out();
        }
        return;
    }
}

/// Click-drag inside the *floating* minimap pans the view away from the
/// player. The pan offset accumulates in tile units and is consumed by
/// `update_minimap_images` on the next frame. Resets to zero whenever
/// the panel re-docks.
pub fn handle_floating_minimap_pan(
    mouse_input: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut pan: ResMut<FloatingMinimapPan>,
    views: Query<(
        &MinimapView,
        &MinimapCanvas,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    title_bars: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<crate::ui::movable_window::MovableWindowTitleBar>,
    >,
    resize_handles: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<crate::ui::movable_window::MovableWindowResizeHandle>,
    >,
) {
    let Some(cursor) = windows
        .iter()
        .next()
        .and_then(|window| window.cursor_position())
    else {
        pan.drag = None;
        return;
    };

    if !mouse_input.pressed(MouseButton::Left) {
        pan.drag = None;
        return;
    }

    if mouse_input.just_pressed(MouseButton::Left) {
        // Don't start a pan if the click landed on a title bar (window
        // drag wins) or a resize handle (resize wins).
        use crate::ui::movable_window::point_in_node;
        let blocked = title_bars
            .iter()
            .any(|(node, t)| point_in_node(cursor, node, t))
            || resize_handles
                .iter()
                .any(|(node, t)| point_in_node(cursor, node, t));
        if !blocked {
            for (view, _canvas, node, transform) in &views {
                if !matches!(view.mode, MinimapMode::FullscreenLarge) {
                    continue;
                }
                if point_in_node(cursor, node, transform) {
                    pan.drag = Some(FloatingMinimapPanDrag {
                        start_cursor: cursor,
                        start_offset: pan.offset_tiles,
                    });
                    break;
                }
            }
        }
    }

    let Some(drag) = pan.drag else { return };

    // Use the current FullscreenLarge view to compute the
    // pixels-per-tile ratio, so panning stays consistent even if the
    // window is resized mid-drag. `node.size()` is in *physical* pixels;
    // scale to logical pixels to match the cursor delta.
    for (view, canvas, node, _transform) in &views {
        if !matches!(view.mode, MinimapMode::FullscreenLarge) {
            continue;
        }
        let inv = node.inverse_scale_factor();
        let size_logical = node.size() * if inv > 0.0 { inv } else { 1.0 };
        let zoom = canvas.last_zoom.unwrap_or_default();
        let span = zoom.tile_span() as f32;
        if size_logical.x <= 0.0 || size_logical.y <= 0.0 || span <= 0.0 {
            return;
        }
        let tile_px_x = size_logical.x / span;
        let tile_px_y = size_logical.y / span;
        let delta = cursor - drag.start_cursor;
        // Grab-and-pan convention: dragging the minimap right reveals
        // what was off-screen to the west (lower x); dragging down
        // reveals what was off-screen to the north (higher y). Game
        // y-axis is positive-north, screen y is positive-down, hence
        // the sign flip on x but not y.
        pan.offset_tiles = Vec2::new(
            drag.start_offset.x - delta.x / tile_px_x,
            drag.start_offset.y + delta.y / tile_px_y,
        );
        return;
    }
}

/// Reset the floating-minimap pan offset whenever the panel is not in
/// floating mode. Keeps the docked HUD minimap centred on the player and
/// makes re-undocking start from a fresh view.
pub fn reset_floating_minimap_pan_when_mounted(
    minimap_mode: Res<MinimapPanelMode>,
    mut pan: ResMut<FloatingMinimapPan>,
) {
    if matches!(minimap_mode.0, PanelMountMode::Mounted) {
        if pan.offset_tiles != Vec2::ZERO || pan.drag.is_some() {
            pan.offset_tiles = Vec2::ZERO;
            pan.drag = None;
        }
    }
}
