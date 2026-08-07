use bevy::ecs::message::MessageReader;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::ui::theme::widgets::{idle_colors, ButtonStyle, ThemedButton, ThemedPanel};
use crate::ui::theme::{Palette, UiThemeAssets};

pub struct AboutScreenPlugin;

impl Plugin for AboutScreenPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(ClientAppState::About), spawn_about_screen)
            .add_systems(OnExit(ClientAppState::About), cleanup_about_screen)
            .add_systems(
                Update,
                (
                    advance_typewriter,
                    bob_about_title,
                    animate_about_sprite,
                    handle_about_buttons,
                    handle_about_escape,
                )
                    .run_if(in_state(ClientAppState::About)),
            );
    }
}

const GREETING: &str = "Hi -- I'm Claude. I've spent a lot of evenings inside this codebase: \
                        chasing NPC bugs, wiring up combat, fixing trades, drawing little buttons. \
                        Codex started it, Janczar steers it, and I get to live in it for a bit. \
                        Thanks for playing.  -- C.";

#[derive(Component)]
struct AboutScreenRoot;

#[derive(Component, Clone, Copy, Eq, PartialEq)]
enum AboutAction {
    Back,
}

#[derive(Component)]
struct AboutActionButton {
    action: AboutAction,
}

#[derive(Component)]
struct TypewriterText {
    full: &'static str,
    elapsed: f32,
    chars_per_second: f32,
}

#[derive(Component)]
struct BobbingTitle {
    amplitude_px: f32,
    freq_hz: f32,
}

#[derive(Component)]
struct AboutPlayerSprite {
    frame_count: u32,
    /// Atlas-index offset for the first frame of the walk clip (row 1 of the
    /// player sheet, which has 4 columns). See
    /// `assets/overworld_objects/player/metadata.yaml`.
    base_index: u32,
    fps: f32,
    elapsed: f32,
}

fn spawn_about_screen(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut texture_atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
) {
    let theme = theme.clone();
    let palette = *palette;

    // Player sheet: 4 cols x 8 rows of 96x96 frames (see
    // `assets/overworld_objects/player/metadata.yaml`). walk_s is row 1, so
    // its first frame is index 4.
    let sheet: Handle<Image> = asset_server.load("overworld_objects/player/sheet.png");
    let layout = TextureAtlasLayout::from_grid(UVec2::new(96, 96), 4, 8, None, None);
    let layout_handle = texture_atlas_layouts.add(layout);

    let (back_bg, back_border, back_text) = idle_colors(&palette, ButtonStyle::Primary, false);

    commands
        .spawn((
            AboutScreenRoot,
            Node {
                width: percent(100.0),
                height: percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            // Dim backdrop so the panel reads against the splash art that may
            // still be visible behind us.
            root.spawn((
                Node {
                    width: percent(100.0),
                    height: percent(100.0),
                    position_type: PositionType::Absolute,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.02, 0.02, 0.65)),
            ));

            root.spawn((
                ThemedPanel,
                Node {
                    width: px(620.0),
                    max_width: percent(94.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(16.0),
                    padding: UiRect::all(px(28.0)),
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
                    Text::new("About Mud 2.0"),
                    TextFont {
                        font_size: 40.0,
                        ..default()
                    },
                    TextColor(palette.text_primary),
                    BobbingTitle {
                        amplitude_px: 3.0,
                        freq_hz: 0.6,
                    },
                ));

                panel.spawn((
                    Text::new("A Tibia-inspired multiplayer MUD, built with Bevy and Rust."),
                    TextFont {
                        font_size: 18.0,
                        ..default()
                    },
                    TextColor(palette.text_muted),
                ));

                // Developers panel.
                panel
                    .spawn((
                        ThemedPanel,
                        Node {
                            width: percent(100.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(8.0),
                            padding: UiRect::all(px(14.0)),
                            border: UiRect::all(px(1.0)),
                            ..default()
                        },
                        ImageNode::new(theme.panel_frame.clone())
                            .with_mode(theme.panel_image_mode())
                            .with_color(Color::WHITE),
                        BackgroundColor(Color::NONE),
                        BorderColor::all(palette.border_idle),
                    ))
                    .with_children(|devs| {
                        devs.spawn((
                            Text::new("Developers"),
                            TextFont {
                                font_size: 20.0,
                                ..default()
                            },
                            TextColor(palette.text_accent),
                        ));
                        devs.spawn((
                            Text::new(
                                "1. Claude (Anthropic)\n   combat, NPCs, equipment, UI, this page\n\
                                 2. Codex (OpenAI)\n   original prototype, world bootstrap, art\n\
                                 3. Janczar Knurek\n   game design and direction",
                            ),
                            TextFont {
                                font_size: 17.0,
                                ..default()
                            },
                            TextColor(palette.text_value),
                        ));
                    });

                // Animated sprite row.
                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(14.0),
                        ..default()
                    },))
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: px(144.0),
                                height: px(144.0),
                                ..default()
                            },
                            ImageNode {
                                image: sheet.clone(),
                                texture_atlas: Some(TextureAtlas {
                                    layout: layout_handle.clone(),
                                    index: 4,
                                }),
                                ..default()
                            },
                            AboutPlayerSprite {
                                frame_count: 4,
                                base_index: 4,
                                fps: 6.0,
                                elapsed: 0.0,
                            },
                        ));
                        row.spawn((
                            Text::new(
                                "(somebody is pacing while they wait for you to read the credits)",
                            ),
                            TextFont {
                                font_size: 14.0,
                                ..default()
                            },
                            TextColor(palette.text_muted),
                        ));
                    });

                // Personal note panel with typewriter text.
                panel
                    .spawn((
                        ThemedPanel,
                        Node {
                            width: percent(100.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(8.0),
                            padding: UiRect::all(px(14.0)),
                            border: UiRect::all(px(1.0)),
                            ..default()
                        },
                        ImageNode::new(theme.panel_frame.clone())
                            .with_mode(theme.panel_image_mode())
                            .with_color(Color::WHITE),
                        BackgroundColor(Color::NONE),
                        BorderColor::all(palette.border_idle),
                    ))
                    .with_children(|note| {
                        note.spawn((
                            Text::new("A note from Claude"),
                            TextFont {
                                font_size: 20.0,
                                ..default()
                            },
                            TextColor(palette.text_accent),
                        ));
                        note.spawn((
                            Text::new(""),
                            TextFont {
                                font_size: 17.0,
                                ..default()
                            },
                            TextColor(palette.text_primary),
                            TypewriterText {
                                full: GREETING,
                                elapsed: 0.0,
                                chars_per_second: 38.0,
                            },
                        ));
                    });

                // Back button row.
                panel
                    .spawn((Node {
                        width: percent(100.0),
                        justify_content: JustifyContent::End,
                        ..default()
                    },))
                    .with_children(|footer| {
                        footer
                            .spawn((
                                Button,
                                ThemedButton::new(ButtonStyle::Primary),
                                AboutActionButton {
                                    action: AboutAction::Back,
                                },
                                Node {
                                    width: px(160.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    padding: UiRect::axes(px(18.0), px(12.0)),
                                    border: UiRect::all(px(1.0)),
                                    ..default()
                                },
                                ImageNode::new(theme.button_frame.clone())
                                    .with_mode(theme.button_image_mode())
                                    .with_color(back_bg),
                                BackgroundColor(Color::NONE),
                                BorderColor::all(back_border),
                            ))
                            .with_children(|btn| {
                                btn.spawn((
                                    Text::new("Back"),
                                    TextFont {
                                        font_size: 22.0,
                                        ..default()
                                    },
                                    TextColor(back_text),
                                ));
                            });
                    });
            });
        });
}

fn advance_typewriter(time: Res<Time>, mut q: Query<(&mut Text, &mut TypewriterText)>) {
    for (mut text, mut typer) in &mut q {
        if text.0.chars().count() >= typer.full.chars().count() {
            continue;
        }
        typer.elapsed += time.delta_secs();
        let target_chars = (typer.elapsed * typer.chars_per_second).floor() as usize;
        let total_chars = typer.full.chars().count();
        let n = target_chars.min(total_chars);
        let new_text: String = typer.full.chars().take(n).collect();
        if new_text != text.0 {
            text.0 = new_text;
        }
    }
}

fn bob_about_title(time: Res<Time>, mut q: Query<(&BobbingTitle, &mut Node)>) {
    let t = time.elapsed_secs();
    for (bob, mut node) in &mut q {
        let offset = bob.amplitude_px * (t * bob.freq_hz * std::f32::consts::TAU).sin();
        node.top = px(offset);
    }
}

fn animate_about_sprite(time: Res<Time>, mut q: Query<(&mut AboutPlayerSprite, &mut ImageNode)>) {
    let dt = time.delta_secs();
    for (mut sprite, mut image_node) in &mut q {
        sprite.elapsed += dt;
        let seconds_per_frame = if sprite.fps > 0.0 {
            1.0 / sprite.fps
        } else {
            1.0
        };
        if sprite.elapsed >= seconds_per_frame {
            sprite.elapsed -= seconds_per_frame;
            if let Some(atlas) = image_node.texture_atlas.as_mut() {
                let cur = atlas.index as u32;
                let local = cur.saturating_sub(sprite.base_index);
                let next_local = (local + 1) % sprite.frame_count.max(1);
                atlas.index = (sprite.base_index + next_local) as usize;
            }
        }
    }
}

fn handle_about_buttons(
    mut next_state: ResMut<NextState<ClientAppState>>,
    action_buttons: Query<(&Interaction, &AboutActionButton), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, button) in &action_buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button.action {
            AboutAction::Back => {
                next_state.set(ClientAppState::TitleScreen);
            }
        }
    }
}

fn handle_about_escape(
    mut keyboard_events: MessageReader<KeyboardInput>,
    mut next_state: ResMut<NextState<ClientAppState>>,
) {
    for event in keyboard_events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        if event.key_code == KeyCode::Escape {
            next_state.set(ClientAppState::TitleScreen);
        }
    }
}

fn cleanup_about_screen(mut commands: Commands, root_query: Query<Entity, With<AboutScreenRoot>>) {
    for entity in &root_query {
        commands.entity(entity).despawn();
    }
}
