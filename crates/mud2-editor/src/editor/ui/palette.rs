#![allow(clippy::type_complexity)]
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::window::PrimaryWindow;

use crate::editor::resources::{EditorState, EditorTool};
use crate::editor::ui::style::{BUTTON_BORDER, HEADER_TEXT, PANEL_BG, PANEL_BORDER};
use mud2::world::floor_definitions::{FloorFlavor, FloorTilesetDefinitions};
use mud2::world::object_definitions::OverworldObjectDefinitions;

#[derive(Component)]
pub struct EditorPaletteRoot;

/// Marks one of the two scrollable list bodies inside the palette panel.
#[derive(Component, Clone, Copy, Debug)]
pub enum EditorScrollableList {
    Objects,
    Floors,
}

#[derive(Component, Clone)]
pub struct EditorPaletteItem {
    pub type_id: String,
    /// Display name for filter matching.
    pub display_name: String,
}

#[derive(Component)]
pub struct EditorPaletteFilterBox;

/// Marks a row in the floor-tile palette. `floor_id = None` is the eraser.
#[derive(Component, Clone)]
pub struct EditorFloorPaletteItem {
    pub floor_id: Option<String>,
}

/// One button in the floor-flavor toggle strip. Selecting it sets
/// `EditorState.selected_floor_flavor`, which is folded into the painted floor
/// id by `EditorState::selected_floor_painted_id`.
#[derive(Component, Clone, Copy)]
pub struct EditorFloorFlavorToggle {
    pub flavor: FloorFlavor,
}

/// Marker on the "Recent" strip's object-row container so a live-updating
/// system can refresh it without rebuilding the entire palette panel.
#[derive(Component)]
pub struct EditorRecentObjectsRoot;

/// Marker on the recent-floor strip's row container.
#[derive(Component)]
pub struct EditorRecentFloorsRoot;

/// Tag for a category header in the object list. `name` is `None` for the
/// "Uncategorized" bucket.
#[derive(Component)]
pub struct EditorPaletteCategoryHeader {
    pub name: Option<String>,
}

// ─── Shared palette core ─────────────────────────────────────────────────────
//
// Everything in this section is host-agnostic: it is parameterized over a
// `PaletteHost` resource (the editor's `EditorState`, the asset viewer's
// `ViewerState`) and over marker components, so both palettes share one
// implementation of the filter box, row spawning, and click plumbing.

/// Background of every palette row button (list rows, recent rows, floor rows).
pub(crate) const PALETTE_ROW_BG: Color = Color::srgba(0.10, 0.07, 0.06, 0.80);
/// Border of every palette row button.
pub(crate) const PALETTE_ROW_BORDER: Color = Color::srgb(0.20, 0.15, 0.10);
/// Label color of palette rows and toggles.
pub(crate) const PALETTE_ROW_TEXT: Color = Color::srgb(0.88, 0.84, 0.78);

/// State a palette panel needs from its host resource: the filter text and
/// its focus flag.
pub trait PaletteHost: Resource {
    /// Placeholder shown when the filter is empty and unfocused.
    const FILTER_PLACEHOLDER: &'static str;

    fn filter(&self) -> &str;
    fn filter_focused(&self) -> bool;
    fn set_filter_focused(&mut self, focused: bool);

    /// How a non-empty filter renders while the box is unfocused.
    fn unfocused_filter_display(&self) -> String {
        self.filter().to_owned()
    }
}

/// A clickable palette row: an id plus a display name, both matched against
/// the filter text.
pub trait PaletteRowItem: Component {
    fn item_id(&self) -> &str;
    fn item_display_name(&self) -> &str;

    /// `filter` must already be lowercased.
    fn matches_filter(&self, filter: &str) -> bool {
        filter.is_empty()
            || self.item_id().to_lowercase().contains(filter)
            || self.item_display_name().to_lowercase().contains(filter)
    }
}

/// Host-specific reaction to a click on a palette row. The shared plumbing
/// ([`process_palette_clicks`]) unfocuses the filter first, then delegates.
pub trait PaletteClickHost<I: PaletteRowItem>: PaletteHost {
    fn palette_item_clicked(&mut self, item: &I);
}

impl PaletteHost for EditorState {
    const FILTER_PLACEHOLDER: &'static str = "filter...";

    fn filter(&self) -> &str {
        &self.palette_filter
    }

    fn filter_focused(&self) -> bool {
        self.palette_filter_focused
    }

    fn set_filter_focused(&mut self, focused: bool) {
        self.palette_filter_focused = focused;
    }
}

impl PaletteRowItem for EditorPaletteItem {
    fn item_id(&self) -> &str {
        &self.type_id
    }

    fn item_display_name(&self) -> &str {
        &self.display_name
    }
}

impl PaletteClickHost<EditorPaletteItem> for EditorState {
    fn palette_item_clicked(&mut self, item: &EditorPaletteItem) {
        // Clicking an object palette item switches back to the object brush,
        // so selection in this list is always active immediately.
        self.current_tool = EditorTool::Brush;
        if self.selected_type_id.as_deref() == Some(&item.type_id) {
            self.selected_type_id = None;
        } else {
            let id = item.type_id.clone();
            self.selected_type_id = Some(id.clone());
            self.selected_object_id = None;
            self.touch_recent_object(&id);
        }
    }
}

/// The `(background, border)` pair for a palette row (or toggle) given its
/// interaction and selection state.
pub fn palette_row_colors(interaction: Interaction, selected: bool) -> (Color, Color) {
    match (interaction, selected) {
        (Interaction::Pressed, _) => (Color::srgb(0.50, 0.28, 0.12), Color::srgb(0.98, 0.84, 0.58)),
        (Interaction::Hovered, true) => {
            (Color::srgb(0.35, 0.20, 0.10), Color::srgb(0.98, 0.84, 0.58))
        }
        (Interaction::Hovered, false) => {
            (Color::srgb(0.20, 0.13, 0.10), Color::srgb(0.60, 0.45, 0.28))
        }
        (Interaction::None, true) => (Color::srgb(0.28, 0.16, 0.08), Color::srgb(0.90, 0.76, 0.50)),
        (Interaction::None, false) => (PALETTE_ROW_BG, PALETTE_ROW_BORDER),
    }
}

/// The `(background, border)` pair for the filter box.
pub fn filter_box_colors(interaction: Interaction, focused: bool) -> (Color, Color) {
    if focused {
        (
            Color::srgba(0.12, 0.08, 0.06, 0.95),
            Color::srgb(0.90, 0.72, 0.40),
        )
    } else {
        match interaction {
            Interaction::Hovered => (
                Color::srgba(0.12, 0.08, 0.06, 0.95),
                Color::srgb(0.50, 0.38, 0.22),
            ),
            _ => (
                Color::srgba(0.08, 0.05, 0.05, 0.90),
                Color::srgb(0.25, 0.18, 0.12),
            ),
        }
    }
}

/// Spawns the clickable filter row: `Button + marker` with a placeholder
/// label. `flex_shrink` is forwarded so each host keeps its existing layout
/// behavior (the editor lets the row shrink, the viewer pins it).
pub fn spawn_filter_row(
    parent: &mut ChildSpawnerCommands,
    marker: impl Bundle,
    placeholder: &str,
    flex_shrink: f32,
) {
    let (bg, border) = filter_box_colors(Interaction::None, false);
    parent
        .spawn((
            marker,
            Button,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                align_items: AlignItems::Center,
                flex_shrink,
                ..default()
            },
            BackgroundColor(bg),
            BorderColor::all(border),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(placeholder.to_owned()),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.50, 0.46, 0.42)),
            ));
        });
}

/// Per-host styling knobs for [`spawn_palette_row`].
#[derive(Clone, Copy)]
pub struct PaletteRowStyle {
    pub swatch_px: f32,
    /// Draw a 1px [`BUTTON_BORDER`] outline around the swatch (floor rows).
    pub swatch_border: bool,
    pub row_pad_y: f32,
    pub label_font_size: f32,
    pub label_flex_grow: f32,
}

impl PaletteRowStyle {
    /// Full-height list rows (editor object/floor lists).
    pub const fn list(swatch_border: bool) -> Self {
        Self {
            swatch_px: 12.0,
            swatch_border,
            row_pad_y: 5.0,
            label_font_size: 11.0,
            label_flex_grow: 0.0,
        }
    }

    /// Compact rows in the editor's "Recent" strips.
    pub const fn recent(swatch_border: bool) -> Self {
        Self {
            swatch_px: 10.0,
            swatch_border,
            row_pad_y: 4.0,
            label_font_size: 11.0,
            label_flex_grow: 0.0,
        }
    }
}

/// Spawns one palette row: `Button + marker`, a color swatch, and a clipped
/// text label.
pub fn spawn_palette_row(
    parent: &mut ChildSpawnerCommands,
    marker: impl Bundle,
    label: &str,
    swatch_color: Color,
    style: PaletteRowStyle,
) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(style.row_pad_y)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(PALETTE_ROW_BG),
            BorderColor::all(PALETTE_ROW_BORDER),
        ))
        .with_children(|btn| {
            let mut swatch_node = Node {
                width: Val::Px(style.swatch_px),
                height: Val::Px(style.swatch_px),
                flex_shrink: 0.0,
                ..default()
            };
            if style.swatch_border {
                swatch_node.border = UiRect::all(Val::Px(1.0));
            }
            let mut swatch = btn.spawn((swatch_node, BackgroundColor(swatch_color)));
            if style.swatch_border {
                swatch.insert(BorderColor::all(BUTTON_BORDER));
            }
            btn.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: style.label_font_size,
                    ..default()
                },
                TextColor(PALETTE_ROW_TEXT),
                Node {
                    overflow: Overflow::clip_x(),
                    flex_grow: style.label_flex_grow,
                    ..default()
                },
            ));
        });
}

/// Renders the filter box's text from the host state: `text_` while focused,
/// the placeholder while empty, otherwise the host's unfocused rendering.
pub fn sync_palette_filter_text_generic<H: PaletteHost, M: Component>(
    state: Res<H>,
    filter_box: Query<Entity, With<M>>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    if !state.is_changed() {
        return;
    }
    let Ok(box_entity) = filter_box.single() else {
        return;
    };
    let Ok(kids) = children.get(box_entity) else {
        return;
    };
    for child in kids.iter() {
        if let Ok(mut text) = texts.get_mut(child) {
            text.0 = if state.filter_focused() {
                format!("{}_", state.filter())
            } else if state.filter().is_empty() {
                H::FILTER_PLACEHOLDER.to_owned()
            } else {
                state.unfocused_filter_display()
            };
        }
    }
}

/// Focuses the filter box when it is clicked.
pub fn handle_palette_filter_click_generic<H: PaletteHost, M: Component>(
    filter_btn: Query<&Interaction, (Changed<Interaction>, With<M>)>,
    mut state: ResMut<H>,
) {
    for interaction in &filter_btn {
        if *interaction == Interaction::Pressed {
            state.set_filter_focused(true);
        }
    }
}

/// Shared body of the palette-row click handlers: a press unfocuses the
/// filter, then hands the row to the host. Takes the `ResMut` itself so
/// change detection only fires on an actual press.
pub fn process_palette_clicks<'a, H, I>(
    state: &mut ResMut<H>,
    items: impl IntoIterator<Item = (&'a I, &'a Interaction)>,
) where
    H: PaletteClickHost<I>,
    I: PaletteRowItem + 'a,
{
    for (item, interaction) in items {
        if *interaction == Interaction::Pressed {
            let state = &mut **state;
            state.set_filter_focused(false);
            state.palette_item_clicked(item);
        }
    }
}

/// Generic palette click system for hosts without extra gating (the editor
/// gates on pick-modes and wraps [`process_palette_clicks`] itself).
pub fn handle_palette_item_clicks<H, I>(
    mut state: ResMut<H>,
    items: Query<(&I, &Interaction), (Changed<Interaction>, With<Button>)>,
) where
    H: PaletteClickHost<I>,
    I: PaletteRowItem,
{
    process_palette_clicks(&mut state, items.iter());
}

/// Small darker header row used above list sections ("Recent", category
/// names).
fn spawn_list_header(parent: &mut ChildSpawnerCommands, marker: impl Bundle, label: &str) {
    parent
        .spawn((
            marker,
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.03, 0.02, 0.85)),
        ))
        .with_children(|h| {
            h.spawn((
                Text::new(label.to_owned()),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.70, 0.55, 0.34)),
            ));
        });
}

// ─── Editor palette panel ────────────────────────────────────────────────────

pub fn spawn_palette_panel(
    parent: &mut ChildSpawnerCommands,
    definitions: &OverworldObjectDefinitions,
    floor_defs: &FloorTilesetDefinitions,
) {
    parent
        .spawn((
            EditorPaletteRoot,
            Node {
                width: Val::Px(200.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                border: UiRect::right(Val::Px(1.0)),
                overflow: Overflow::clip_y(),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(PANEL_BORDER),
        ))
        .with_children(|panel| {
            // Header
            panel
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(PANEL_BORDER),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Objects"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(HEADER_TEXT),
                    ));
                });

            // Filter row (flex_shrink 1.0 = the row may shrink with the panel,
            // matching the editor's historical layout).
            spawn_filter_row(
                panel,
                EditorPaletteFilterBox,
                EditorState::FILTER_PLACEHOLDER,
                1.0,
            );

            // "Recent" strip — populated/refreshed by `sync_recent_strip`
            // each frame from `EditorState.recent_object_types`.
            panel.spawn((
                EditorRecentObjectsRoot,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    ..default()
                },
            ));

            // Scrollable object list — shares remaining vertical space 50/50
            // with the floors list below via matching `flex_grow`. `min_height:
            // 0` lets flexbox actually shrink the list inside its scroll
            // viewport instead of letting its natural content height push the
            // floors section past the panel's bottom.
            panel
                .spawn((
                    EditorScrollableList::Objects,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                ))
                .with_children(|list| {
                    // Group objects by `category`. `None` falls into the
                    // "Uncategorized" bucket which renders last. Within each
                    // group rows are alphabetical.
                    let mut by_category: std::collections::BTreeMap<Option<String>, Vec<&str>> =
                        std::collections::BTreeMap::new();
                    for id in definitions.ids() {
                        if let Some(def) = definitions.get(id) {
                            by_category
                                .entry(def.category.clone())
                                .or_default()
                                .push(id);
                        }
                    }
                    // Build a stable ordering: named categories alphabetically
                    // first, then the catch-all None bucket.
                    let mut groups: Vec<(Option<String>, Vec<&str>)> =
                        by_category.into_iter().collect();
                    groups.sort_by(|a, b| match (&a.0, &b.0) {
                        (Some(x), Some(y)) => x.cmp(y),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    });

                    for (category, mut ids) in groups {
                        ids.sort();
                        let header_label = category
                            .clone()
                            .unwrap_or_else(|| "Uncategorized".to_owned());
                        spawn_list_header(
                            list,
                            EditorPaletteCategoryHeader {
                                name: category.clone(),
                            },
                            &header_label,
                        );

                        for type_id in ids {
                            let Some(def) = definitions.get(type_id) else {
                                continue;
                            };
                            spawn_palette_row(
                                list,
                                EditorPaletteItem {
                                    type_id: type_id.to_owned(),
                                    display_name: def.name.clone(),
                                },
                                &def.name,
                                def.debug_color(),
                                PaletteRowStyle::list(false),
                            );
                        }
                    }
                });

            // Floors header
            panel
                .spawn((
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        border: UiRect::axes(Val::Px(0.0), Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(PANEL_BORDER),
                ))
                .with_children(|h| {
                    h.spawn((
                        Text::new("Floors  (B = object brush)"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(HEADER_TEXT),
                    ));
                });

            // Flavor toggle: applies a programmatic treatment (e.g. Flooring)
            // to whatever tileset the floor brush paints.
            panel
                .spawn((Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    flex_shrink: 0.0,
                    ..default()
                },))
                .with_children(|row| {
                    row.spawn((
                        Text::new("Flavor:"),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.78, 0.74, 0.68)),
                    ));
                    for &flavor in FloorFlavor::ALL {
                        spawn_flavor_toggle(row, flavor);
                    }
                });

            // Recent floors strip — same shape as recent objects.
            panel.spawn((
                EditorRecentFloorsRoot,
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    ..default()
                },
            ));

            // Floor list (Erase + each FloorTilesetDefinition). Same flex
            // sizing as the objects list so they share remaining height 50/50.
            panel
                .spawn((
                    EditorScrollableList::Floors,
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    ScrollPosition::default(),
                ))
                .with_children(|list| {
                    spawn_floor_row(list, None, "Erase", Color::srgba(0.0, 0.0, 0.0, 0.0));

                    let mut floor_defs_sorted: Vec<
                        &mud2::world::floor_definitions::FloorTilesetDefinition,
                    > = floor_defs.iter().collect();
                    floor_defs_sorted
                        .sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));

                    for def in floor_defs_sorted {
                        spawn_floor_row(list, Some(def.id.clone()), &def.name, def.debug_color());
                    }
                });
        });
}

fn spawn_floor_row(
    list: &mut ChildSpawnerCommands,
    floor_id: Option<String>,
    label: &str,
    swatch_color: Color,
) {
    spawn_palette_row(
        list,
        EditorFloorPaletteItem { floor_id },
        label,
        swatch_color,
        PaletteRowStyle::list(true),
    );
}

/// Spawns one button in the floor-flavor toggle strip. Active state is painted
/// by [`sync_floor_flavor_toggle`].
fn spawn_flavor_toggle(row: &mut ChildSpawnerCommands, flavor: FloorFlavor) {
    row.spawn((
        Button,
        EditorFloorFlavorToggle { flavor },
        Node {
            padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(PALETTE_ROW_BG),
        BorderColor::all(PALETTE_ROW_BORDER),
    ))
    .with_children(|btn| {
        btn.spawn((
            Text::new(flavor.label()),
            TextFont {
                font_size: 11.0,
                ..default()
            },
            TextColor(PALETTE_ROW_TEXT),
        ));
    });
}

/// Sets `selected_floor_flavor` when a flavor toggle is clicked.
pub fn handle_floor_flavor_toggle_clicks(
    mut editor_state: ResMut<EditorState>,
    items: Query<(&EditorFloorFlavorToggle, &Interaction), (Changed<Interaction>, With<Button>)>,
) {
    for (item, interaction) in &items {
        if *interaction == Interaction::Pressed {
            editor_state.palette_filter_focused = false;
            editor_state.selected_floor_flavor = item.flavor;
        }
    }
}

/// Highlights the active flavor toggle each frame.
pub fn sync_floor_flavor_toggle(
    editor_state: Res<EditorState>,
    mut items: Query<
        (
            &EditorFloorFlavorToggle,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Button>,
    >,
) {
    for (item, interaction, mut bg, mut border) in &mut items {
        let selected = item.flavor == editor_state.selected_floor_flavor;
        let (bg_color, border_color) = palette_row_colors(*interaction, selected);
        bg.0 = bg_color;
        *border = BorderColor::all(border_color);
    }
}

pub fn sync_palette_selection(
    editor_state: Res<EditorState>,
    mut items: Query<
        (
            &EditorPaletteItem,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            &mut Node,
        ),
        With<Button>,
    >,
    mut filter_box: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (With<EditorPaletteFilterBox>, Without<EditorPaletteItem>),
    >,
) {
    let filter = editor_state.palette_filter.to_lowercase();
    let filter_focused = editor_state.palette_filter_focused;

    for (item, interaction, mut bg, mut border, mut node) in &mut items {
        // Hide non-matching rows from layout (not just from rendering) so the
        // remaining items collapse to the top of the list.
        let matches = item.matches_filter(&filter);
        let target_display = if matches {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != target_display {
            node.display = target_display;
        }

        if !matches {
            continue;
        }

        let is_selected = editor_state
            .selected_type_id
            .as_deref()
            .is_some_and(|id| id == item.type_id);
        let (bg_color, border_color) = palette_row_colors(*interaction, is_selected);
        bg.0 = bg_color;
        *border = BorderColor::all(border_color);
    }

    // Sync filter box appearance
    for (interaction, mut bg, mut border) in &mut filter_box {
        let (b, br) = filter_box_colors(*interaction, filter_focused);
        bg.0 = b;
        *border = BorderColor::all(br);
    }
}

pub fn sync_palette_filter_text(
    editor_state: Res<EditorState>,
    filter_box: Query<Entity, With<EditorPaletteFilterBox>>,
    children: Query<&Children>,
    texts: Query<&mut Text>,
) {
    sync_palette_filter_text_generic(editor_state, filter_box, children, texts);
}

pub fn handle_palette_filter_click(
    filter_btn: Query<&Interaction, (Changed<Interaction>, With<EditorPaletteFilterBox>)>,
    editor_state: ResMut<EditorState>,
) {
    handle_palette_filter_click_generic(filter_btn, editor_state);
}

pub fn handle_palette_clicks(
    mut editor_state: ResMut<EditorState>,
    items: Query<(&EditorPaletteItem, &Interaction), (Changed<Interaction>, With<Button>)>,
    vendor_stash_buffer: Res<crate::editor::resources::EditorVendorStashBuffer>,
    contents_buffer: Res<crate::editor::resources::EditorContentsBuffer>,
) {
    // Vendor-stash ware picking and container-contents picking have priority:
    // when either arm is set, palette clicks belong to that flow (handled by
    // `handle_vendor_stash_palette_pick` / `handle_contents_palette_pick`),
    // not the object brush. Without this gate, picking would also arm the brush.
    if vendor_stash_buffer.pending_ware_pick.is_some()
        || contents_buffer.pending_item_pick.is_some()
    {
        return;
    }
    process_palette_clicks(&mut editor_state, items.iter());
}

pub fn handle_floor_palette_clicks(
    mut editor_state: ResMut<EditorState>,
    items: Query<(&EditorFloorPaletteItem, &Interaction), (Changed<Interaction>, With<Button>)>,
) {
    for (item, interaction) in &items {
        if *interaction == Interaction::Pressed {
            editor_state.palette_filter_focused = false;
            editor_state.current_tool = EditorTool::FloorBrush;
            editor_state.selected_floor_type = item.floor_id.clone();
            if let Some(id) = item.floor_id.as_deref() {
                editor_state.touch_recent_floor(id);
            }
        }
    }
}

pub fn handle_palette_scrolling(
    mut mouse_wheel: MessageReader<MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut lists: Query<(
        &EditorScrollableList,
        &Node,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
    )>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    // ComputedNode geometry is in physical pixels; logical cursor → physical.
    let cursor = cursor * window.scale_factor();

    for event in mouse_wheel.read() {
        let mut delta_y = -event.y;
        if event.unit == MouseScrollUnit::Line {
            delta_y *= 21.0;
        }
        if delta_y == 0.0 {
            continue;
        }
        for (_marker, node, computed, transform, mut scroll_position) in &mut lists {
            if !computed.contains_point(*transform, cursor) {
                continue;
            }
            if node.overflow.y != bevy::ui::OverflowAxis::Scroll {
                continue;
            }
            let max_offset =
                (computed.content_size() - computed.size()) * computed.inverse_scale_factor();
            if max_offset.y <= 0.0 {
                continue;
            }
            scroll_position.y = (scroll_position.y + delta_y).clamp(0.0, max_offset.y);
            break;
        }
    }
}

/// Marker on a recent-strip row so children clicks can switch the palette
/// selection just like the full-list rows do. We reuse `EditorPaletteItem`
/// and `EditorFloorPaletteItem` directly so existing handlers fire.
#[derive(Component)]
pub struct EditorRecentRow;

/// Tag the strip container so the Recent rebuild knows which entity to
/// despawn children of. Stored as a component since the recent VecDeque
/// changes incrementally and a per-frame rebuild is cheap (≤ 12 rows).
pub fn sync_recent_strip(
    mut commands: Commands,
    editor_state: Res<EditorState>,
    definitions: Res<OverworldObjectDefinitions>,
    floor_defs: Res<FloorTilesetDefinitions>,
    objects_root: Query<Entity, With<EditorRecentObjectsRoot>>,
    floors_root: Query<Entity, With<EditorRecentFloorsRoot>>,
    existing_rows: Query<Entity, With<EditorRecentRow>>,
    parents: Query<&ChildOf>,
    mut last_signature: Local<Option<(Vec<String>, Vec<String>)>>,
) {
    let current_signature = (
        editor_state
            .recent_object_types
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
        editor_state
            .recent_floor_types
            .iter()
            .cloned()
            .collect::<Vec<_>>(),
    );
    if last_signature.as_ref() == Some(&current_signature) {
        return;
    }
    *last_signature = Some(current_signature.clone());

    let Ok(objects_root) = objects_root.single() else {
        return;
    };
    let Ok(floors_root) = floors_root.single() else {
        return;
    };

    // Despawn old rows under either strip.
    for entity in &existing_rows {
        if let Ok(parent) = parents.get(entity) {
            let p = parent.parent();
            if p == objects_root || p == floors_root {
                commands.entity(entity).despawn();
            }
        }
    }

    if !editor_state.recent_object_types.is_empty() {
        commands.entity(objects_root).with_children(|root| {
            spawn_list_header(root, EditorRecentRow, "Recent");

            for type_id in &current_signature.0 {
                let Some(def) = definitions.get(type_id) else {
                    continue;
                };
                spawn_palette_row(
                    root,
                    (
                        EditorRecentRow,
                        EditorPaletteItem {
                            type_id: type_id.clone(),
                            display_name: def.name.clone(),
                        },
                    ),
                    &def.name,
                    def.debug_color(),
                    PaletteRowStyle::recent(false),
                );
            }
        });
    }

    if !editor_state.recent_floor_types.is_empty() {
        commands.entity(floors_root).with_children(|root| {
            spawn_list_header(root, EditorRecentRow, "Recent");

            for floor_id in &current_signature.1 {
                let Some(def) = floor_defs.iter().find(|d| d.id == *floor_id) else {
                    continue;
                };
                spawn_palette_row(
                    root,
                    (
                        EditorRecentRow,
                        EditorFloorPaletteItem {
                            floor_id: Some(floor_id.clone()),
                        },
                    ),
                    &def.name,
                    def.debug_color(),
                    PaletteRowStyle::recent(true),
                );
            }
        });
    }
}

pub fn sync_floor_palette_selection(
    editor_state: Res<EditorState>,
    mut items: Query<
        (
            &EditorFloorPaletteItem,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<Button>,
    >,
) {
    let active_floor_tool = editor_state.current_tool == EditorTool::FloorBrush;
    for (item, interaction, mut bg, mut border) in &mut items {
        let is_selected = active_floor_tool && editor_state.selected_floor_type == item.floor_id;
        let (bg_color, border_color) = palette_row_colors(*interaction, is_selected);
        bg.0 = bg_color;
        *border = BorderColor::all(border_color);
    }
}
