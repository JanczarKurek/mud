//! Character Create screen. Name + class + 6-attribute point-buy. Sends
//! `ClientMessage::CreateCharacter` on submit; on success returns to
//! `CharacterSelect` (the roster refresh comes from the server-issued
//! `CharacterList`).

use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::log::{info, warn};
use bevy::prelude::*;

use crate::app::plugin::AppRuntime;
use crate::app::state::ClientAppState;
use crate::network::protocol::{ClientMessage, ServerMessage};
use crate::network::resources::{TcpClientConfig, TcpClientConnection};
use crate::player::classes::Class;
use crate::player::components::{
    validate_point_buy, AppearanceRegion, AttributeSet, PlayerAppearance, RgbColor, ATTR_BASELINE,
    ATTR_CEILING, ATTR_FLOOR, POINT_BUY_BUDGET,
};
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton, ThemedPanel};
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct CharacterCreateScreenPlugin {
    pub runtime: AppRuntime,
}

impl Plugin for CharacterCreateScreenPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CharacterCreateState::new(self.runtime))
            .add_systems(
                OnEnter(ClientAppState::CharacterCreate),
                (reset_create_form, spawn_character_create_screen).chain(),
            )
            .add_systems(
                Update,
                (
                    handle_name_field_keyboard,
                    handle_class_buttons,
                    handle_attr_buttons,
                    handle_hue_buttons,
                    handle_create_actions,
                    sync_form_text,
                    animate_preview_layers,
                    poll_create_result,
                )
                    .run_if(in_state(ClientAppState::CharacterCreate)),
            )
            .add_systems(
                OnExit(ClientAppState::CharacterCreate),
                cleanup_character_create_screen,
            );
    }
}

#[derive(Resource)]
pub struct CharacterCreateState {
    pub runtime: AppRuntime,
    pub name: String,
    pub class: Class,
    pub attributes: AttributeSet,
    pub appearance: PlayerAppearance,
    pub hair_hue: f32,
    pub torso_hue: f32,
    pub trousers_hue: f32,
    pub error_message: Option<String>,
}

/// Hue starting points roughly matching `PlayerAppearance::default()` so the
/// initial swatches don't jump when the screen first renders.
const DEFAULT_HAIR_HUE: f32 = 30.0;
const DEFAULT_TORSO_HUE: f32 = 215.0;
const DEFAULT_TROUSERS_HUE: f32 = 240.0;
const HUE_STEP_DEGREES: f32 = 15.0;

impl CharacterCreateState {
    fn new(runtime: AppRuntime) -> Self {
        Self {
            runtime,
            name: String::new(),
            class: Class::Fighter,
            attributes: AttributeSet::new(
                ATTR_BASELINE,
                ATTR_BASELINE,
                ATTR_BASELINE,
                ATTR_BASELINE,
                ATTR_BASELINE,
                ATTR_BASELINE,
            ),
            appearance: PlayerAppearance::default(),
            hair_hue: DEFAULT_HAIR_HUE,
            torso_hue: DEFAULT_TORSO_HUE,
            trousers_hue: DEFAULT_TROUSERS_HUE,
            error_message: None,
        }
    }

    fn refresh_appearance(&mut self) {
        self.appearance.hair = RgbColor::from_hue(self.hair_hue);
        self.appearance.torso = RgbColor::from_hue(self.torso_hue);
        self.appearance.trousers = RgbColor::from_hue(self.trousers_hue);
    }

    fn hue_for(&self, region: AppearanceRegion) -> f32 {
        match region {
            AppearanceRegion::Hair => self.hair_hue,
            AppearanceRegion::Torso => self.torso_hue,
            AppearanceRegion::Trousers => self.trousers_hue,
            AppearanceRegion::Skin => 0.0,
        }
    }

    fn set_hue(&mut self, region: AppearanceRegion, hue: f32) {
        let wrapped = hue.rem_euclid(360.0);
        match region {
            AppearanceRegion::Hair => self.hair_hue = wrapped,
            AppearanceRegion::Torso => self.torso_hue = wrapped,
            AppearanceRegion::Trousers => self.trousers_hue = wrapped,
            AppearanceRegion::Skin => {}
        }
    }
}

fn reset_create_form(mut state: ResMut<CharacterCreateState>) {
    state.name.clear();
    state.class = Class::Fighter;
    state.attributes = AttributeSet::new(
        ATTR_BASELINE,
        ATTR_BASELINE,
        ATTR_BASELINE,
        ATTR_BASELINE,
        ATTR_BASELINE,
        ATTR_BASELINE,
    );
    state.hair_hue = DEFAULT_HAIR_HUE;
    state.torso_hue = DEFAULT_TORSO_HUE;
    state.trousers_hue = DEFAULT_TROUSERS_HUE;
    state.refresh_appearance();
    state.error_message = None;
}

#[derive(Component)]
struct CharacterCreateRoot;

#[derive(Component)]
struct NameFieldText;

#[derive(Component, Clone, Copy)]
struct ClassButton(Class);

#[derive(Component)]
struct ClassButtonText;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Attribute {
    Strength,
    Agility,
    Constitution,
    Willpower,
    Charisma,
    Focus,
}

const ATTRIBUTES: [(Attribute, &str); 6] = [
    (Attribute::Strength, "Strength"),
    (Attribute::Agility, "Agility"),
    (Attribute::Constitution, "Constitution"),
    (Attribute::Willpower, "Willpower"),
    (Attribute::Charisma, "Charisma"),
    (Attribute::Focus, "Focus"),
];

#[derive(Component, Clone, Copy)]
struct AttrAdjustButton {
    attribute: Attribute,
    delta: i32,
}

#[derive(Component, Clone, Copy)]
struct AttrValueText(Attribute);

#[derive(Component, Clone, Copy)]
struct HueAdjustButton {
    region: AppearanceRegion,
    /// Sign-only: actual step size is `HUE_STEP_DEGREES`.
    sign: i32,
}

#[derive(Component, Clone, Copy)]
struct HueValueText(AppearanceRegion);

#[derive(Component, Clone, Copy)]
struct HueSwatch(AppearanceRegion);

const HUE_ROWS: [(AppearanceRegion, &str); 3] = [
    (AppearanceRegion::Hair, "Hair Color"),
    (AppearanceRegion::Torso, "Torso Color"),
    (AppearanceRegion::Trousers, "Trousers Color"),
];

/// Tag for one layer of the appearance preview shown next to the hue rows.
/// `None` is the un-tinted base sheet; `Some(region)` is a tintable overlay
/// whose color tracks the corresponding `CharacterCreateState` slider.
#[derive(Component, Clone, Copy)]
struct PreviewLayer(Option<AppearanceRegion>);

/// Drives idle-clip frame cycling on every preview-layer `ImageNode`. All
/// layers share the same fps + frame count, so independent timers stay in
/// lockstep for the short lifetime of the Create screen.
#[derive(Component)]
struct PreviewAnimation {
    elapsed: f32,
    fps: f32,
    frame_count: u32,
    base_index: u32,
}

const PREVIEW_FRAME_W: u32 = 96;
const PREVIEW_FRAME_H: u32 = 96;
const PREVIEW_SHEET_COLS: u32 = 4;
const PREVIEW_SHEET_ROWS: u32 = 8;
const PREVIEW_SCALE: f32 = 2.0;
const PREVIEW_FPS: f32 = 3.0;
const PREVIEW_IDLE_FRAMES: u32 = 4;

#[derive(Component)]
struct PointsRemainingText;

#[derive(Component)]
struct CreateErrorText;

#[derive(Component, Clone, Copy, Eq, PartialEq)]
enum CreateAction {
    Submit,
    Cancel,
}

#[derive(Component)]
struct CreateActionButton(CreateAction);

fn attr_value(attrs: &AttributeSet, attribute: Attribute) -> i32 {
    match attribute {
        Attribute::Strength => attrs.strength,
        Attribute::Agility => attrs.agility,
        Attribute::Constitution => attrs.constitution,
        Attribute::Willpower => attrs.willpower,
        Attribute::Charisma => attrs.charisma,
        Attribute::Focus => attrs.focus,
    }
}

fn set_attr_value(attrs: &mut AttributeSet, attribute: Attribute, value: i32) {
    match attribute {
        Attribute::Strength => attrs.strength = value,
        Attribute::Agility => attrs.agility = value,
        Attribute::Constitution => attrs.constitution = value,
        Attribute::Willpower => attrs.willpower = value,
        Attribute::Charisma => attrs.charisma = value,
        Attribute::Focus => attrs.focus = value,
    }
}

fn points_spent(attrs: &AttributeSet) -> i32 {
    (attrs.strength - ATTR_BASELINE)
        + (attrs.agility - ATTR_BASELINE)
        + (attrs.constitution - ATTR_BASELINE)
        + (attrs.willpower - ATTR_BASELINE)
        + (attrs.charisma - ATTR_BASELINE)
        + (attrs.focus - ATTR_BASELINE)
}

fn spawn_character_create_screen(
    mut commands: Commands,
    state: Res<CharacterCreateState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let theme = theme.clone();
    let palette = *palette;

    // Player sheet atlas: must match `assets/overworld_objects/player/metadata.yaml`
    // (96×96 frames, 4 cols × 8 rows). Shared across the base sheet + every
    // tintable layer ImageNode so they index the same frame each tick.
    let preview_layout = TextureAtlasLayout::from_grid(
        UVec2::new(PREVIEW_FRAME_W, PREVIEW_FRAME_H),
        PREVIEW_SHEET_COLS,
        PREVIEW_SHEET_ROWS,
        None,
        None,
    );
    let preview_layout_handle = texture_atlas_layouts.add(preview_layout);
    let preview_base_image: Handle<Image> = asset_server.load("overworld_objects/player/sheet.png");
    let preview_hair_image: Handle<Image> =
        asset_server.load("overworld_objects/player/layers/hair.png");
    let preview_torso_image: Handle<Image> =
        asset_server.load("overworld_objects/player/layers/torso.png");
    let preview_trousers_image: Handle<Image> =
        asset_server.load("overworld_objects/player/layers/trousers.png");

    commands
        .spawn((
            CharacterCreateRoot,
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.03, 0.03, 0.96)),
        ))
        .with_children(|root| {
            root.spawn((
                ThemedPanel,
                Node {
                    width: px(640.0),
                    max_width: percent(96.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(14.0),
                    padding: UiRect::all(px(24.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                ImageNode::new(theme.panel_frame.clone())
                    .with_mode(theme.panel_image_mode())
                    .with_color(Color::WHITE),
                BackgroundColor(Color::NONE),
                BorderColor::all(palette.border_accent),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Create a character"),
                    TextFont {
                        font_size: 32.0,
                        ..default()
                    },
                    TextColor(palette.text_primary),
                ));

                // Name input.
                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4.0),
                        ..default()
                    },))
                    .with_children(|section| {
                        section.spawn((
                            Text::new("Name"),
                            TextFont {
                                font_size: 16.0,
                                ..default()
                            },
                            TextColor(palette.text_accent),
                        ));
                        section
                            .spawn((
                                Node {
                                    width: percent(100.0),
                                    height: px(32.0),
                                    padding: UiRect::axes(px(10.0), px(6.0)),
                                    border: UiRect::all(px(1.0)),
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.08, 0.08, 0.08, 0.65)),
                                BorderColor::all(palette.border_accent),
                            ))
                            .with_children(|inner| {
                                inner.spawn((
                                    NameFieldText,
                                    Text::new(state.name.clone()),
                                    TextFont {
                                        font_size: 18.0,
                                        ..default()
                                    },
                                    TextColor(palette.text_primary),
                                ));
                            });
                    });

                // Class picker.
                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6.0),
                        ..default()
                    },))
                    .with_children(|section| {
                        section.spawn((
                            Text::new("Class"),
                            TextFont {
                                font_size: 16.0,
                                ..default()
                            },
                            TextColor(palette.text_accent),
                        ));
                        section
                            .spawn((Node {
                                width: percent(100.0),
                                column_gap: px(8.0),
                                ..default()
                            },))
                            .with_children(|row| {
                                for class in Class::ALL {
                                    let selected = class == state.class;
                                    spawn_class_button(row, &theme, &palette, class, selected);
                                }
                            });
                        section.spawn((
                            Text::new(class_blurb(state.class)),
                            ClassButtonText,
                            TextFont {
                                font_size: 13.0,
                                ..default()
                            },
                            TextColor(palette.text_muted),
                        ));
                    });

                // Attributes — header + 6 rows.
                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6.0),
                        ..default()
                    },))
                    .with_children(|section| {
                        section
                            .spawn((Node {
                                width: percent(100.0),
                                justify_content: JustifyContent::SpaceBetween,
                                ..default()
                            },))
                            .with_children(|header| {
                                header.spawn((
                                    Text::new("Attributes"),
                                    TextFont {
                                        font_size: 16.0,
                                        ..default()
                                    },
                                    TextColor(palette.text_accent),
                                ));
                                header.spawn((
                                    PointsRemainingText,
                                    Text::new(points_remaining_label(&state.attributes)),
                                    TextFont {
                                        font_size: 14.0,
                                        ..default()
                                    },
                                    TextColor(palette.text_muted),
                                ));
                            });
                        for (attribute, label) in ATTRIBUTES {
                            spawn_attr_row(
                                section,
                                &theme,
                                &palette,
                                attribute,
                                label,
                                attr_value(&state.attributes, attribute),
                            );
                        }
                    });

                // Appearance — hair / torso / trousers hue sliders + live
                // preview of the player sprite stacked on the right.
                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6.0),
                        ..default()
                    },))
                    .with_children(|section| {
                        section.spawn((
                            Text::new("Appearance"),
                            TextFont {
                                font_size: 16.0,
                                ..default()
                            },
                            TextColor(palette.text_accent),
                        ));
                        section
                            .spawn((Node {
                                width: percent(100.0),
                                flex_direction: FlexDirection::Row,
                                column_gap: px(16.0),
                                align_items: AlignItems::FlexStart,
                                ..default()
                            },))
                            .with_children(|row| {
                                row.spawn((Node {
                                    flex_direction: FlexDirection::Column,
                                    flex_grow: 1.0,
                                    row_gap: px(6.0),
                                    ..default()
                                },))
                                    .with_children(|hues| {
                                        for (region, label) in HUE_ROWS {
                                            let hue = state.hue_for(region);
                                            spawn_hue_row(
                                                hues, &theme, &palette, region, label, hue,
                                            );
                                        }
                                    });
                                spawn_appearance_preview(
                                    row,
                                    &palette,
                                    &state,
                                    preview_layout_handle.clone(),
                                    preview_base_image.clone(),
                                    preview_hair_image.clone(),
                                    preview_torso_image.clone(),
                                    preview_trousers_image.clone(),
                                );
                            });
                    });

                panel.spawn((
                    CreateErrorText,
                    Text::new(state.error_message.clone().unwrap_or_default()),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.95, 0.35, 0.35)),
                ));

                panel
                    .spawn((Node {
                        width: percent(100.0),
                        column_gap: px(12.0),
                        ..default()
                    },))
                    .with_children(|actions| {
                        spawn_action_button(
                            actions,
                            &theme,
                            &palette,
                            "Create",
                            CreateAction::Submit,
                        );
                        spawn_action_button(
                            actions,
                            &theme,
                            &palette,
                            "Cancel",
                            CreateAction::Cancel,
                        );
                    });
            });
        });
}

fn spawn_class_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    class: Class,
    selected: bool,
) {
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Slot, selected);
    parent
        .spawn((
            Button,
            ThemedButton {
                style: ButtonStyle::Slot,
                selected,
            },
            ClassButton(class),
            Node {
                flex_grow: 1.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(12.0), px(10.0)),
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
                Text::new(class.label()),
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

fn spawn_attr_row(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    attribute: Attribute,
    label: &str,
    value: i32,
) {
    parent
        .spawn((Node {
            width: percent(100.0),
            column_gap: px(8.0),
            align_items: AlignItems::Center,
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                Node {
                    width: px(120.0),
                    ..default()
                },
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(palette.text_primary),
            ));

            spawn_small_button(
                row,
                theme,
                palette,
                "-",
                AttrAdjustButton {
                    attribute,
                    delta: -1,
                },
            );

            row.spawn((
                AttrValueText(attribute),
                Text::new(value.to_string()),
                Node {
                    width: px(32.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(palette.text_value),
            ));

            spawn_small_button(
                row,
                theme,
                palette,
                "+",
                AttrAdjustButton {
                    attribute,
                    delta: 1,
                },
            );
        });
}

fn spawn_hue_row(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    region: AppearanceRegion,
    label: &str,
    hue: f32,
) {
    let swatch_color = RgbColor::from_hue(hue).to_bevy();
    parent
        .spawn((Node {
            width: percent(100.0),
            column_gap: px(8.0),
            align_items: AlignItems::Center,
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                Node {
                    width: px(120.0),
                    ..default()
                },
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(palette.text_primary),
            ));

            spawn_small_button(
                row,
                theme,
                palette,
                "-",
                HueAdjustButton { region, sign: -1 },
            );

            row.spawn((
                HueValueText(region),
                Text::new(hue_label(hue)),
                Node {
                    width: px(48.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                TextFont {
                    font_size: 16.0,
                    ..default()
                },
                TextColor(palette.text_value),
            ));

            spawn_small_button(
                row,
                theme,
                palette,
                "+",
                HueAdjustButton { region, sign: 1 },
            );

            row.spawn((
                HueSwatch(region),
                Node {
                    width: px(28.0),
                    height: px(28.0),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(swatch_color),
                BorderColor::all(palette.border_accent),
            ));
        });
}

fn hue_label(hue: f32) -> String {
    format!("{:03}°", hue.rem_euclid(360.0).round() as i32)
}

#[allow(clippy::too_many_arguments)]
fn spawn_appearance_preview(
    parent: &mut ChildSpawnerCommands,
    palette: &Palette,
    state: &CharacterCreateState,
    layout_handle: Handle<TextureAtlasLayout>,
    base_image: Handle<Image>,
    hair_image: Handle<Image>,
    torso_image: Handle<Image>,
    trousers_image: Handle<Image>,
) {
    let preview_w = PREVIEW_FRAME_W as f32 * PREVIEW_SCALE;
    let preview_h = PREVIEW_FRAME_H as f32 * PREVIEW_SCALE;

    parent
        .spawn((
            Node {
                width: px(preview_w),
                height: px(preview_h),
                flex_shrink: 0.0,
                position_type: PositionType::Relative,
                border: UiRect::all(px(1.0)),
                padding: UiRect::all(px(0.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.08, 0.07, 0.07, 0.65)),
            BorderColor::all(palette.border_accent),
        ))
        .with_children(|stack| {
            let spawn_layer = |stack: &mut ChildSpawnerCommands,
                               image: Handle<Image>,
                               marker: PreviewLayer,
                               tint: Color| {
                stack.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100.0),
                        height: percent(100.0),
                        top: px(0.0),
                        left: px(0.0),
                        ..default()
                    },
                    ImageNode {
                        image,
                        texture_atlas: Some(TextureAtlas {
                            layout: layout_handle.clone(),
                            index: 0,
                        }),
                        color: tint,
                        ..default()
                    },
                    marker,
                    PreviewAnimation {
                        elapsed: 0.0,
                        fps: PREVIEW_FPS,
                        frame_count: PREVIEW_IDLE_FRAMES,
                        base_index: 0,
                    },
                ));
            };

            spawn_layer(stack, base_image, PreviewLayer(None), Color::WHITE);
            spawn_layer(
                stack,
                hair_image,
                PreviewLayer(Some(AppearanceRegion::Hair)),
                state.appearance.hair.to_bevy(),
            );
            spawn_layer(
                stack,
                torso_image,
                PreviewLayer(Some(AppearanceRegion::Torso)),
                state.appearance.torso.to_bevy(),
            );
            spawn_layer(
                stack,
                trousers_image,
                PreviewLayer(Some(AppearanceRegion::Trousers)),
                state.appearance.trousers.to_bevy(),
            );
        });
}

fn spawn_small_button<M: Component>(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    marker: M,
) {
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Secondary, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Secondary),
            marker,
            Node {
                width: px(28.0),
                height: px(28.0),
                justify_content: JustifyContent::Center,
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
                    font_size: 18.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

fn spawn_action_button(
    parent: &mut ChildSpawnerCommands,
    theme: &UiThemeAssets,
    palette: &Palette,
    label: &str,
    action: CreateAction,
) {
    let (bg, border, text) = idle_colors(palette, ButtonStyle::Primary, false);
    parent
        .spawn((
            Button,
            ThemedButton::new(ButtonStyle::Primary),
            CreateActionButton(action),
            Node {
                flex_grow: 1.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(14.0), px(12.0)),
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
                    font_size: 18.0,
                    ..default()
                },
                TextColor(text),
            ));
        });
}

fn handle_name_field_keyboard(
    mut state: ResMut<CharacterCreateState>,
    mut keyboard_events: bevy::ecs::message::MessageReader<KeyboardInput>,
) {
    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match event.key_code {
            KeyCode::Backspace => {
                state.name.pop();
            }
            _ => {
                if event.repeat {
                    continue;
                }
                if let Key::Character(character) = &event.logical_key {
                    for ch in character.chars() {
                        if !ch.is_control() && state.name.chars().count() < 24 {
                            state.name.push(ch);
                        }
                    }
                }
            }
        }
    }
}

fn handle_class_buttons(
    mut state: ResMut<CharacterCreateState>,
    buttons: Query<(&Interaction, &ClassButton), (Changed<Interaction>, With<Button>)>,
    mut all_buttons: Query<(&ClassButton, &mut ThemedButton)>,
) {
    let mut chose = None;
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            chose = Some(button.0);
        }
    }
    if let Some(class) = chose {
        state.class = class;
        for (button, mut themed) in &mut all_buttons {
            themed.selected = button.0 == class;
        }
    }
}

fn handle_attr_buttons(
    mut state: ResMut<CharacterCreateState>,
    buttons: Query<(&Interaction, &AttrAdjustButton), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let current = attr_value(&state.attributes, button.attribute);
        let target = current + button.delta;
        if !(ATTR_FLOOR..=ATTR_CEILING).contains(&target) {
            continue;
        }
        // Reject increases that would exceed the budget.
        if button.delta > 0 {
            let spent = points_spent(&state.attributes);
            if spent + button.delta > POINT_BUY_BUDGET {
                continue;
            }
        }
        set_attr_value(&mut state.attributes, button.attribute, target);
    }
}

fn animate_preview_layers(
    time: Res<Time>,
    mut layers: Query<(&mut PreviewAnimation, &mut ImageNode)>,
) {
    let dt = time.delta_secs();
    for (mut anim, mut image_node) in &mut layers {
        anim.elapsed += dt;
        let seconds_per_frame = if anim.fps > 0.0 { 1.0 / anim.fps } else { 1.0 };
        if anim.elapsed >= seconds_per_frame {
            anim.elapsed -= seconds_per_frame;
            if let Some(atlas) = image_node.texture_atlas.as_mut() {
                let cur = atlas.index as u32;
                let local = cur.saturating_sub(anim.base_index);
                let next_local = (local + 1) % anim.frame_count.max(1);
                atlas.index = (anim.base_index + next_local) as usize;
            }
        }
    }
}

fn handle_hue_buttons(
    mut state: ResMut<CharacterCreateState>,
    buttons: Query<(&Interaction, &HueAdjustButton), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let current = state.hue_for(button.region);
        let target = current + button.sign as f32 * HUE_STEP_DEGREES;
        state.set_hue(button.region, target);
        state.refresh_appearance();
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_form_text(
    state: Res<CharacterCreateState>,
    mut name_text: Query<&mut Text, With<NameFieldText>>,
    mut attr_text: Query<(&AttrValueText, &mut Text), Without<NameFieldText>>,
    mut points_text: Query<
        &mut Text,
        (
            With<PointsRemainingText>,
            Without<NameFieldText>,
            Without<AttrValueText>,
        ),
    >,
    mut error_text: Query<
        &mut Text,
        (
            With<CreateErrorText>,
            Without<NameFieldText>,
            Without<AttrValueText>,
            Without<PointsRemainingText>,
        ),
    >,
    mut class_blurb_text: Query<
        &mut Text,
        (
            With<ClassButtonText>,
            Without<NameFieldText>,
            Without<AttrValueText>,
            Without<PointsRemainingText>,
            Without<CreateErrorText>,
        ),
    >,
    mut hue_text: Query<
        (&HueValueText, &mut Text),
        (
            Without<NameFieldText>,
            Without<AttrValueText>,
            Without<PointsRemainingText>,
            Without<CreateErrorText>,
            Without<ClassButtonText>,
        ),
    >,
    mut hue_swatches: Query<(&HueSwatch, &mut BackgroundColor)>,
    mut preview_layers: Query<(&PreviewLayer, &mut ImageNode)>,
) {
    for mut text in &mut name_text {
        if text.0 != state.name {
            text.0 = state.name.clone();
        }
    }
    for (marker, mut text) in &mut attr_text {
        let value = attr_value(&state.attributes, marker.0).to_string();
        if text.0 != value {
            text.0 = value;
        }
    }
    let label = points_remaining_label(&state.attributes);
    for mut text in &mut points_text {
        if text.0 != label {
            text.0 = label.clone();
        }
    }
    let err = state.error_message.clone().unwrap_or_default();
    for mut text in &mut error_text {
        if text.0 != err {
            text.0 = err.clone();
        }
    }
    let blurb = class_blurb(state.class).to_owned();
    for mut text in &mut class_blurb_text {
        if text.0 != blurb {
            text.0 = blurb.clone();
        }
    }
    for (marker, mut text) in &mut hue_text {
        let label = hue_label(state.hue_for(marker.0));
        if text.0 != label {
            text.0 = label;
        }
    }
    for (marker, mut bg) in &mut hue_swatches {
        let color = match marker.0 {
            AppearanceRegion::Hair => state.appearance.hair,
            AppearanceRegion::Torso => state.appearance.torso,
            AppearanceRegion::Trousers => state.appearance.trousers,
            AppearanceRegion::Skin => RgbColor::new(255, 255, 255),
        };
        bg.0 = color.to_bevy();
    }
    for (marker, mut image_node) in &mut preview_layers {
        let tint = match marker.0 {
            None => Color::WHITE,
            Some(AppearanceRegion::Hair) => state.appearance.hair.to_bevy(),
            Some(AppearanceRegion::Torso) => state.appearance.torso.to_bevy(),
            Some(AppearanceRegion::Trousers) => state.appearance.trousers.to_bevy(),
            Some(AppearanceRegion::Skin) => Color::WHITE,
        };
        if image_node.color != tint {
            image_node.color = tint;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_create_actions(
    mut state: ResMut<CharacterCreateState>,
    mut next_state: ResMut<NextState<ClientAppState>>,
    config: Option<Res<TcpClientConfig>>,
    mut connection: Option<ResMut<TcpClientConnection>>,
    db: Option<Res<crate::accounts::AccountDbHandle>>,
    action_buttons: Query<
        (&Interaction, &CreateActionButton),
        (Changed<Interaction>, With<Button>),
    >,
) {
    for (interaction, button) in &action_buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button.0 {
            CreateAction::Submit => {
                if state.name.trim().len() < 3 {
                    state.error_message =
                        Some("character name must be at least 3 characters".to_owned());
                    continue;
                }
                if let Err(msg) = validate_point_buy(&state.attributes) {
                    state.error_message = Some(msg);
                    continue;
                }
                state.error_message = None;

                match state.runtime {
                    AppRuntime::TcpClient => {
                        let msg = ClientMessage::CreateCharacter {
                            name: state.name.trim().to_owned(),
                            class: state.class,
                            attributes: state.attributes,
                            appearance: state.appearance,
                        };
                        if let (Some(config), Some(connection)) =
                            (config.as_deref(), connection.as_deref_mut())
                        {
                            crate::network::systems::ensure_tcp_client_connected(
                                config, connection,
                            );
                            if let Some(stream) = connection.stream.as_mut() {
                                let mut disconnected = false;
                                crate::network::systems::write_message(
                                    stream,
                                    &msg,
                                    &mut disconnected,
                                );
                                if disconnected {
                                    connection.stream = None;
                                    connection.read_buffer.clear();
                                    state.error_message = Some("connection lost".to_owned());
                                    continue;
                                }
                            }
                        }
                        info!("sent CreateCharacter for {}", state.name);
                        // `poll_create_result` transitions on the server reply.
                    }
                    AppRuntime::EmbeddedClient => {
                        let Some(db) = db.as_deref() else {
                            state.error_message = Some("no local account database".to_owned());
                            continue;
                        };
                        let result = {
                            let mut guard = db.lock();
                            guard.create_character(
                                crate::accounts::LOCAL_ACCOUNT_ID,
                                state.name.trim(),
                                state.class,
                                state.attributes,
                                state.appearance,
                            )
                        };
                        match result {
                            Ok(character_id) => {
                                info!("embedded: created character {character_id}");
                                next_state.set(ClientAppState::CharacterSelect);
                            }
                            Err(err) => {
                                state.error_message = Some(err.to_string());
                            }
                        }
                    }
                    AppRuntime::HeadlessServer => {}
                }
            }
            CreateAction::Cancel => {
                next_state.set(ClientAppState::CharacterSelect);
            }
        }
    }
}

fn poll_create_result(
    mut state: ResMut<CharacterCreateState>,
    mut next_state: ResMut<NextState<ClientAppState>>,
    config: Option<Res<TcpClientConfig>>,
    mut connection: Option<ResMut<TcpClientConnection>>,
) {
    let (Some(config), Some(connection)) = (config, connection.as_deref_mut()) else {
        return;
    };
    crate::network::systems::ensure_tcp_client_connected(&config, connection);
    let mut read_buffer = std::mem::take(&mut connection.read_buffer);
    let Some(stream) = connection.stream.as_mut() else {
        connection.read_buffer = read_buffer;
        return;
    };

    let mut disconnected = false;
    let mut succeeded = false;
    while let Some(line) =
        crate::network::systems::read_next_line(stream, &mut read_buffer, &mut disconnected)
    {
        match serde_json::from_str::<ServerMessage>(&line) {
            Ok(ServerMessage::CharacterCreateResult {
                ok,
                character_id,
                reason,
            }) => {
                if ok {
                    info!("character created: {:?}", character_id);
                    state.error_message = None;
                    succeeded = true;
                } else {
                    let reason = reason.unwrap_or_else(|| "create rejected".to_owned());
                    warn!("character create rejected: {reason}");
                    state.error_message = Some(reason);
                }
            }
            Ok(_) => {}
            Err(err) => warn!("character create: failed to parse message: {err}"),
        }
    }

    if disconnected {
        connection.stream = None;
        connection.read_buffer.clear();
        state.error_message = Some("connection lost".to_owned());
    } else {
        connection.read_buffer = read_buffer;
    }

    if succeeded {
        next_state.set(ClientAppState::CharacterSelect);
    }
}

fn cleanup_character_create_screen(
    mut commands: Commands,
    root_query: Query<Entity, With<CharacterCreateRoot>>,
) {
    for entity in &root_query {
        commands.entity(entity).despawn();
    }
}

fn class_blurb(class: Class) -> &'static str {
    match class {
        Class::Fighter => "Fighter — d10 HP. Front-line martial. Hits hard, soaks hits.",
        Class::Wizard => "Wizard — d4 HP. Arcane caster, mana-rich, scales hard.",
        Class::Cleric => "Cleric — d8 HP. Divine caster, mid martial, full healer.",
        Class::Vagabond => "Vagabond — d6 HP. Skill specialist, opportunistic damage.",
    }
}

fn points_remaining_label(attrs: &AttributeSet) -> String {
    let spent = points_spent(attrs);
    let remaining = POINT_BUY_BUDGET - spent;
    format!("Points remaining: {remaining}/{POINT_BUY_BUDGET}")
}
