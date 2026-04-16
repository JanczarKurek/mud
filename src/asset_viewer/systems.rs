#![allow(clippy::type_complexity)]
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::asset_viewer::resources::{AssetKind, InspectorBuffer, PreviewState, ViewerState};
use crate::world::animation::AnimatedSprite;
use crate::world::object_definitions::OverworldObjectDefinitions;

pub const PREVIEW_TILE_SIZE: f32 = 96.0;

/// Marker for the 2D camera owned by the asset viewer.
#[derive(Component)]
pub struct AssetViewerCamera;

/// Marker on the preview sprite entity.
#[derive(Component)]
pub struct PreviewMarker {
    pub definition_id: String,
}

pub fn setup_viewer_camera(mut commands: Commands) {
    commands.spawn((Camera2d, AssetViewerCamera));
}

pub fn update_preview(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    viewer_state: Res<ViewerState>,
    object_defs: Res<OverworldObjectDefinitions>,
    mut preview_state: ResMut<PreviewState>,
    mut inspector_buffer: ResMut<InspectorBuffer>,
    mut last_selection: Local<(Option<String>, AssetKind)>,
) {
    if !viewer_state.is_changed() { return; }
    let current = (viewer_state.selected_id.clone(), viewer_state.selected_kind);
    if *last_selection == current { return; }
    *last_selection = current;

    if let Some(entity) = preview_state.preview_entity.take() {
        commands.entity(entity).despawn();
    }
    preview_state.current_clip = None;

    let Some(id) = &viewer_state.selected_id else {
        *inspector_buffer = InspectorBuffer::default();
        return;
    };

    match viewer_state.selected_kind {
        AssetKind::Object => inspector_buffer.load_object(id),
        AssetKind::Spell => inspector_buffer.load_spell(id),
    }

    if viewer_state.selected_kind == AssetKind::Object {
        if let Some(def) = object_defs.get(id) {
            let size = def.render.sprite_pixel_size(PREVIEW_TILE_SIZE);
            let sprite = if let Some(sprite_path) = &def.render.sprite_path {
                let mut s = Sprite::from_image(asset_server.load(sprite_path));
                s.custom_size = Some(size);
                s.image_mode = bevy::sprite::SpriteImageMode::Auto;
                s
            } else {
                Sprite::from_color(def.debug_color(), size)
            };

            let entity = commands
                .spawn((
                    sprite,
                    Transform::from_xyz(0.0, 0.0, 0.0),
                    PreviewMarker { definition_id: id.clone() },
                ))
                .id();
            preview_state.preview_entity = Some(entity);

            if let Some(sheet) = &def.render.animation {
                preview_state.current_clip = sheet
                    .clips
                    .contains_key("idle")
                    .then(|| "idle".to_string())
                    .or_else(|| sheet.clips.keys().next().cloned());
            }
        }
    }
}

pub fn attach_preview_animation(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    definitions: Res<OverworldObjectDefinitions>,
    query: Query<(Entity, &PreviewMarker), Without<AnimatedSprite>>,
) {
    for (entity, marker) in &query {
        let Some(def) = definitions.get(&marker.definition_id) else { continue };
        let Some(sheet) = &def.render.animation else { continue };

        let layout = TextureAtlasLayout::from_grid(
            UVec2::new(sheet.frame_width, sheet.frame_height),
            sheet.sheet_columns,
            sheet.sheet_rows,
            None,
            None,
        );
        let layout_handle = texture_atlas_layouts.add(layout);
        let image_handle: Handle<Image> = asset_server.load(&sheet.sheet_path);

        let idle = sheet.clips.get("idle");
        let animated = AnimatedSprite {
            current_clip: "idle".to_string(),
            frame_index: 0,
            frame_timer: 0.0,
            frame_count: idle.map_or(1, |c| c.frame_count),
            seconds_per_frame: idle.map_or(1.0, |c| {
                if c.fps > 0.0 { 1.0 / c.fps } else { 1.0 }
            }),
            atlas_columns: sheet.sheet_columns,
            clip_row: idle.map_or(0, |c| c.row),
            clip_start_col: idle.map_or(0, |c| c.start_col),
            looping: idle.is_none_or(|c| c.looping),
        };

        let new_sprite = Sprite {
            image: image_handle,
            custom_size: Some(Vec2::new(sheet.frame_width as f32, sheet.frame_height as f32)),
            texture_atlas: Some(TextureAtlas { layout: layout_handle, index: 0 }),
            ..default()
        };

        commands.entity(entity).insert((animated, new_sprite));
    }
}

pub fn apply_clip_change(
    preview_state: Res<PreviewState>,
    definitions: Res<OverworldObjectDefinitions>,
    mut query: Query<(&PreviewMarker, &mut AnimatedSprite)>,
) {
    if !preview_state.is_changed() { return; }
    let Some(clip_name) = &preview_state.current_clip else { return };

    for (marker, mut animated) in &mut query {
        let Some(def) = definitions.get(&marker.definition_id) else { continue };
        let Some(sheet) = &def.render.animation else { continue };
        let Some(clip) = sheet.clips.get(clip_name) else { continue };

        animated.current_clip = clip_name.clone();
        animated.frame_index = 0;
        animated.frame_timer = 0.0;
        animated.frame_count = clip.frame_count;
        animated.seconds_per_frame = if clip.fps > 0.0 { 1.0 / clip.fps } else { 1.0 };
        animated.clip_row = clip.row;
        animated.clip_start_col = clip.start_col;
        animated.looping = clip.looping;
    }
}

pub fn handle_keyboard(
    mut keyboard_events: bevy::ecs::message::MessageReader<KeyboardInput>,
    mut viewer_state: ResMut<ViewerState>,
    mut inspector_buffer: ResMut<InspectorBuffer>,
) {
    for event in keyboard_events.read() {
        if !event.state.is_pressed() { continue; }

        if viewer_state.filter_focused {
            match event.key_code {
                KeyCode::Escape => { viewer_state.filter_focused = false; }
                KeyCode::Backspace => { viewer_state.filter.pop(); }
                _ => {
                    if event.repeat { continue; }
                    match &event.logical_key {
                        Key::Character(ch) => { viewer_state.filter.push_str(ch.as_str()); }
                        Key::Space => { viewer_state.filter.push(' '); }
                        _ => {}
                    }
                }
            }
        } else if inspector_buffer.editing_index.is_some() {
            match event.key_code {
                KeyCode::Escape => {
                    inspector_buffer.editing_index = None;
                    inspector_buffer.edit_text.clear();
                }
                KeyCode::Enter | KeyCode::Tab => {
                    inspector_buffer.commit_edit();
                }
                KeyCode::Backspace => { inspector_buffer.edit_text.pop(); }
                _ => {
                    if event.repeat { continue; }
                    match &event.logical_key {
                        Key::Character(ch) => { inspector_buffer.edit_text.push_str(ch.as_str()); }
                        Key::Space => { inspector_buffer.edit_text.push(' '); }
                        _ => {}
                    }
                }
            }
        }
    }
}

pub fn handle_viewer_zoom(
    mut scroll_events: bevy::ecs::message::MessageReader<MouseWheel>,
    mut camera_query: Query<&mut Projection, With<AssetViewerCamera>>,
) {
    let Ok(mut proj) = camera_query.single_mut() else { return };
    for event in scroll_events.read() {
        if let Projection::Orthographic(ref mut ortho) = *proj {
            ortho.scale = (ortho.scale - event.y * 0.1).clamp(0.2, 5.0);
        }
    }
}



// ── Palette ───────────────────────────────────────────────────────────────────

pub fn handle_palette_clicks(
    mut viewer_state: ResMut<ViewerState>,
    items: Query<
        (&ViewerPaletteItem, &Interaction),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (item, interaction) in &items {
        if *interaction == Interaction::Pressed {
            viewer_state.filter_focused = false;
            if viewer_state.selected_id.as_deref() == Some(&item.id)
                && viewer_state.selected_kind == item.kind
            {
                viewer_state.selected_id = None;
            } else {
                viewer_state.selected_id = Some(item.id.clone());
                viewer_state.selected_kind = item.kind;
            }
        }
    }
}

pub fn handle_filter_click(
    filter_btn: Query<&Interaction, (Changed<Interaction>, With<ViewerFilterBox>)>,
    mut viewer_state: ResMut<ViewerState>,
) {
    for interaction in &filter_btn {
        if *interaction == Interaction::Pressed {
            viewer_state.filter_focused = true;
        }
    }
}

pub fn handle_tab_clicks(
    mut viewer_state: ResMut<ViewerState>,
    tabs: Query<(&ViewerTab, &Interaction), (Changed<Interaction>, With<Button>)>,
) {
    for (tab, interaction) in &tabs {
        if *interaction == Interaction::Pressed
            && viewer_state.selected_kind != tab.kind
        {
            viewer_state.selected_kind = tab.kind;
            viewer_state.selected_id = None;
        }
    }
}

pub fn sync_palette(
    viewer_state: Res<ViewerState>,
    mut items: Query<(
        &ViewerPaletteItem,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
        &mut Visibility,
    ), With<Button>>,
    mut filter_box: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (With<ViewerFilterBox>, Without<ViewerPaletteItem>),
    >,
) {
    if !viewer_state.is_changed() { return; }
    let filter = viewer_state.filter.to_lowercase();

    for (item, interaction, mut bg, mut border, mut vis) in &mut items {
        if item.kind != viewer_state.selected_kind {
            *vis = Visibility::Hidden;
            continue;
        }
        let matches = filter.is_empty()
            || item.id.to_lowercase().contains(&filter)
            || item.display_name.to_lowercase().contains(&filter);
        *vis = if matches { Visibility::Visible } else { Visibility::Hidden };
        if !matches { continue; }

        let is_selected = viewer_state.selected_id.as_deref() == Some(&item.id);
        let (bg_color, border_color) = match (*interaction, is_selected) {
            (Interaction::Pressed, _) => (Color::srgb(0.50, 0.28, 0.12), Color::srgb(0.98, 0.84, 0.58)),
            (Interaction::Hovered, true) => (Color::srgb(0.35, 0.20, 0.10), Color::srgb(0.98, 0.84, 0.58)),
            (Interaction::Hovered, false) => (Color::srgb(0.20, 0.13, 0.10), Color::srgb(0.60, 0.45, 0.28)),
            (Interaction::None, true) => (Color::srgb(0.28, 0.16, 0.08), Color::srgb(0.90, 0.76, 0.50)),
            (Interaction::None, false) => (Color::srgba(0.10, 0.07, 0.06, 0.80), Color::srgb(0.20, 0.15, 0.10)),
        };
        bg.0 = bg_color;
        *border = BorderColor::all(border_color);
    }

    for (interaction, mut bg, mut border) in &mut filter_box {
        let (b, br) = if viewer_state.filter_focused {
            (Color::srgba(0.12, 0.08, 0.06, 0.95), Color::srgb(0.90, 0.72, 0.40))
        } else {
            match *interaction {
                Interaction::Hovered => (Color::srgba(0.12, 0.08, 0.06, 0.95), Color::srgb(0.50, 0.38, 0.22)),
                _ => (Color::srgba(0.08, 0.05, 0.05, 0.90), Color::srgb(0.25, 0.18, 0.12)),
            }
        };
        bg.0 = b;
        *border = BorderColor::all(br);
    }
}

pub fn sync_filter_text(
    viewer_state: Res<ViewerState>,
    filter_box: Query<Entity, With<ViewerFilterBox>>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    if !viewer_state.is_changed() { return; }
    let Ok(box_entity) = filter_box.single() else { return };
    let Ok(kids) = children.get(box_entity) else { return };
    for child in kids.iter() {
        if let Ok(mut text) = texts.get_mut(child) {
            text.0 = if viewer_state.filter_focused {
                format!("{}_", viewer_state.filter)
            } else if viewer_state.filter.is_empty() {
                "filter…".into()
            } else {
                format!("  {}", viewer_state.filter)
            };
        }
    }
}

pub fn sync_tab_buttons(
    viewer_state: Res<ViewerState>,
    mut tabs: Query<(&ViewerTab, &Interaction, &mut BackgroundColor, &mut BorderColor)>,
) {
    if !viewer_state.is_changed() { return; }
    for (tab, interaction, mut bg, mut border) in &mut tabs {
        let active = tab.kind == viewer_state.selected_kind;
        let (b, br) = match (*interaction, active) {
            (Interaction::Pressed, _) | (_, true) => (Color::srgb(0.28, 0.16, 0.08), Color::srgb(0.90, 0.76, 0.50)),
            (Interaction::Hovered, false) => (Color::srgb(0.15, 0.10, 0.08), Color::srgb(0.50, 0.38, 0.22)),
            _ => (Color::srgba(0.08, 0.05, 0.05, 0.80), Color::srgb(0.20, 0.15, 0.10)),
        };
        bg.0 = b;
        *border = BorderColor::all(br);
    }
}

// ── Inspector ─────────────────────────────────────────────────────────────────

pub fn sync_inspector_panel(
    mut commands: Commands,
    buffer: Res<InspectorBuffer>,
    body_query: Query<Entity, With<InspectorBody>>,
    title_query: Query<Entity, With<InspectorTitle>>,
    children_query: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    if !buffer.is_changed() { return; }
    let Ok(body_entity) = body_query.single() else { return };

    commands.entity(body_entity).despawn_related::<Children>();
    commands.entity(body_entity).with_children(|parent| {
        for (i, field) in buffer.fields.iter().enumerate() {
            let is_editing = buffer.editing_index == Some(i);
            let value_display = if is_editing {
                format!("{}_", buffer.edit_text)
            } else {
                field.display_value.clone()
            };

            let indent = field.display_path.matches('.').count();
            let label = field
                .display_path
                .rsplit('.')
                .next()
                .unwrap_or(&field.display_path)
                .to_string();
            let label_with_indent = format!("{}{}", "  ".repeat(indent), label);

            parent
                .spawn((
                    InspectorRow { index: i },
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(4.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(if is_editing {
                        Color::srgb(0.20, 0.12, 0.06)
                    } else {
                        Color::srgba(0.08, 0.05, 0.05, 0.0)
                    }),
                    BorderColor::all(Color::srgb(0.15, 0.11, 0.08)),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(label_with_indent),
                        TextFont { font_size: 10.0, ..default() },
                        TextColor(Color::srgb(0.70, 0.65, 0.55)),
                        Node { width: Val::Percent(52.0), overflow: Overflow::clip_x(), flex_shrink: 0.0, ..default() },
                    ));
                    row.spawn((
                        Text::new(value_display),
                        TextFont { font_size: 10.0, ..default() },
                        TextColor(if is_editing {
                            Color::srgb(1.0, 0.85, 0.5)
                        } else {
                            Color::srgb(0.90, 0.84, 0.72)
                        }),
                        Node { flex_grow: 1.0, overflow: Overflow::clip_x(), ..default() },
                    ));
                });
        }
    });

    // Sync title
    if let Ok(title_entity) = title_query.single() {
        let label = match &buffer.asset_id {
            Some(id) => {
                let dirty = if buffer.dirty { " *" } else { "" };
                format!("{}{}", id, dirty)
            }
            None => "—".to_string(),
        };
        if let Ok(kids) = children_query.get(title_entity) {
            for child in kids.iter() {
                if let Ok(mut text) = texts.get_mut(child) {
                    text.0 = label.clone();
                }
            }
        }
    }
}

pub fn handle_inspector_row_click(
    items: Query<(&InspectorRow, &Interaction), (Changed<Interaction>, With<Button>)>,
    mut buffer: ResMut<InspectorBuffer>,
) {
    for (row, interaction) in &items {
        if *interaction == Interaction::Pressed {
            if buffer.editing_index == Some(row.index) { continue; }
            // Commit any previous edit first
            if buffer.editing_index.is_some() {
                buffer.commit_edit();
            }
            if let Some(field) = buffer.fields.get(row.index) {
                buffer.edit_text = field.display_value.clone();
                buffer.editing_index = Some(row.index);
            }
        }
    }
}

pub fn handle_save_button(
    buttons: Query<&Interaction, (Changed<Interaction>, With<ViewerSaveButton>)>,
    mut buffer: ResMut<InspectorBuffer>,
) {
    for interaction in &buttons {
        if *interaction == Interaction::Pressed {
            match buffer.save() {
                Ok(()) => bevy::log::info!("Asset saved successfully"),
                Err(e) => bevy::log::error!("Save failed: {}", e),
            }
        }
    }
}

pub fn sync_save_button(
    buffer: Res<InspectorBuffer>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor, &mut BorderColor), With<ViewerSaveButton>>,
) {
    if !buffer.is_changed() { return; }
    for (interaction, mut bg, mut border) in &mut buttons {
        let (b, br) = if buffer.dirty {
            match *interaction {
                Interaction::Pressed => (Color::srgb(0.60, 0.30, 0.10), Color::srgb(1.0, 0.85, 0.5)),
                Interaction::Hovered => (Color::srgb(0.40, 0.22, 0.08), Color::srgb(0.98, 0.80, 0.45)),
                _ => (Color::srgb(0.30, 0.18, 0.06), Color::srgb(0.90, 0.72, 0.38)),
            }
        } else {
            (Color::srgba(0.10, 0.07, 0.06, 0.70), Color::srgb(0.22, 0.16, 0.12))
        };
        bg.0 = b;
        *border = BorderColor::all(br);
    }
}

// ── Clip buttons ──────────────────────────────────────────────────────────────

pub fn sync_clip_buttons(
    mut commands: Commands,
    viewer_state: Res<ViewerState>,
    preview_state: Res<PreviewState>,
    object_defs: Res<OverworldObjectDefinitions>,
    container_query: Query<Entity, With<ClipButtonContainer>>,
    mut last_id: Local<Option<String>>,
) {
    if !viewer_state.is_changed() && !preview_state.is_changed() { return; }
    let current_id = viewer_state.selected_id.clone().filter(|_| viewer_state.selected_kind == AssetKind::Object);
    if *last_id == current_id { return; }
    *last_id = current_id.clone();

    let Ok(container) = container_query.single() else { return };
    commands.entity(container).despawn_related::<Children>();

    let Some(id) = current_id else { return };
    let Some(def) = object_defs.get(&id) else { return };
    let Some(sheet) = &def.render.animation else { return };

    let mut clip_names: Vec<String> = sheet.clips.keys().cloned().collect();
    clip_names.sort();

    commands.entity(container).with_children(|row| {
        for clip_name in clip_names {
            row.spawn((
                Button,
                ClipButton { clip_name: clip_name.clone() },
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.07, 0.06, 0.85)),
                BorderColor::all(Color::srgb(0.30, 0.22, 0.15)),
            ))
            .with_children(|btn| {
                btn.spawn((
                    Text::new(clip_name),
                    TextFont { font_size: 11.0, ..default() },
                    TextColor(Color::srgb(0.88, 0.84, 0.78)),
                ));
            });
        }
    });
}

pub fn handle_clip_button_clicks(
    mut preview_state: ResMut<PreviewState>,
    buttons: Query<(&ClipButton, &Interaction), (Changed<Interaction>, With<Button>)>,
) {
    for (btn, interaction) in &buttons {
        if *interaction == Interaction::Pressed {
            preview_state.current_clip = Some(btn.clip_name.clone());
        }
    }
}

pub fn sync_clip_button_highlight(
    preview_state: Res<PreviewState>,
    mut buttons: Query<(&ClipButton, &Interaction, &mut BackgroundColor, &mut BorderColor)>,
) {
    if !preview_state.is_changed() { return; }
    for (btn, interaction, mut bg, mut border) in &mut buttons {
        let active = preview_state.current_clip.as_deref() == Some(&btn.clip_name);
        let (b, br) = match (*interaction, active) {
            (_, true) => (Color::srgb(0.28, 0.16, 0.08), Color::srgb(0.90, 0.76, 0.50)),
            (Interaction::Hovered, false) => (Color::srgb(0.15, 0.10, 0.08), Color::srgb(0.50, 0.38, 0.22)),
            _ => (Color::srgba(0.10, 0.07, 0.06, 0.85), Color::srgb(0.30, 0.22, 0.15)),
        };
        bg.0 = b;
        *border = BorderColor::all(br);
    }
}

// ── Top bar ───────────────────────────────────────────────────────────────────

pub fn sync_top_bar_title(
    viewer_state: Res<ViewerState>,
    title_query: Query<Entity, With<TopBarTitle>>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    if !viewer_state.is_changed() { return; }
    let Ok(entity) = title_query.single() else { return };
    let label = viewer_state
        .selected_id
        .as_deref()
        .map(|id| format!("Asset Viewer — {}", id))
        .unwrap_or_else(|| "Asset Viewer".to_string());
    if let Ok(kids) = children.get(entity) {
        for child in kids.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                text.0 = label.clone();
            }
        }
    }
}

// ── Components (markers used by systems above) ────────────────────────────────

#[derive(Component, Clone)]
pub struct ViewerPaletteItem {
    pub id: String,
    pub display_name: String,
    pub kind: AssetKind,
}

#[derive(Component)]
pub struct ViewerFilterBox;

#[derive(Component)]
pub struct ViewerTab {
    pub kind: AssetKind,
}

#[derive(Component)]
pub struct InspectorBody;

#[derive(Component)]
pub struct InspectorTitle;

#[derive(Component)]
pub struct InspectorRow {
    pub index: usize,
}

#[derive(Component)]
pub struct ViewerSaveButton;

#[derive(Component)]
pub struct ClipButtonContainer;

#[derive(Component, Clone)]
pub struct ClipButton {
    pub clip_name: String,
}

#[derive(Component)]
pub struct TopBarTitle;
