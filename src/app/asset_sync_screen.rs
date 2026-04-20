use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::network::resources::AssetSyncState;

pub struct AssetSyncScreenPlugin;

impl Plugin for AssetSyncScreenPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(ClientAppState::AssetSync), spawn_asset_sync_screen)
            .add_systems(
                Update,
                (sync_progress_bar, sync_status_text, sync_log_text)
                    .run_if(in_state(ClientAppState::AssetSync)),
            )
            .add_systems(OnExit(ClientAppState::AssetSync), cleanup_asset_sync_screen);
    }
}

#[derive(Component)]
struct AssetSyncRoot;

#[derive(Component)]
struct AssetSyncProgressFill;

#[derive(Component)]
struct AssetSyncStatusText;

#[derive(Component)]
struct AssetSyncLogText;

fn spawn_asset_sync_screen(mut commands: Commands) {
    commands
        .spawn((
            AssetSyncRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.03, 0.03, 0.96)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Px(480.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(16.0),
                    padding: UiRect::all(Val::Px(28.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.08, 0.07, 0.07, 0.92)),
                BorderColor::all(Color::srgba(0.30, 0.25, 0.20, 0.50)),
            ))
            .with_children(|panel| {
                // Title
                panel.spawn((
                    Text::new("Syncing Assets"),
                    TextFont {
                        font_size: 22.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.90, 0.85, 0.70)),
                ));

                // Status line
                panel.spawn((
                    AssetSyncStatusText,
                    Text::new("Connecting to server..."),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.65, 0.60, 0.55)),
                ));

                // Progress bar background
                panel
                    .spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(14.0),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.12, 0.11, 0.10)),
                        BorderColor::all(Color::srgba(0.30, 0.25, 0.20, 0.40)),
                    ))
                    .with_children(|bar| {
                        bar.spawn((
                            AssetSyncProgressFill,
                            Node {
                                width: Val::Percent(0.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.35, 0.65, 0.35)),
                        ));
                    });

                // Log text
                panel.spawn((
                    AssetSyncLogText,
                    Text::new(""),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.50, 0.48, 0.44)),
                ));
            });
        });
}

fn sync_progress_bar(
    sync_state: Res<AssetSyncState>,
    mut fill_query: Query<&mut Node, With<AssetSyncProgressFill>>,
) {
    let pct = if sync_state.total_needed == 0 {
        0.0
    } else {
        sync_state.received_count as f32 / sync_state.total_needed as f32 * 100.0
    };
    for mut node in &mut fill_query {
        node.width = Val::Percent(pct);
    }
}

fn sync_status_text(
    sync_state: Res<AssetSyncState>,
    mut text_query: Query<&mut Text, With<AssetSyncStatusText>>,
) {
    if !sync_state.is_changed() {
        return;
    }
    let msg = if !sync_state.manifest_received {
        "Connecting to server...".to_owned()
    } else if sync_state.total_needed == 0 {
        "All assets up to date.".to_owned()
    } else {
        format!(
            "Downloading {} of {} files...",
            sync_state.received_count, sync_state.total_needed
        )
    };
    for mut text in &mut text_query {
        **text = msg.clone();
    }
}

fn sync_log_text(
    sync_state: Res<AssetSyncState>,
    mut text_query: Query<&mut Text, With<AssetSyncLogText>>,
) {
    if !sync_state.is_changed() {
        return;
    }
    let lines = sync_state.log_messages.iter().rev().take(12).rev();
    let content = lines.cloned().collect::<Vec<_>>().join("\n");
    for mut text in &mut text_query {
        **text = content.clone();
    }
}

fn cleanup_asset_sync_screen(
    mut commands: Commands,
    root_query: Query<Entity, With<AssetSyncRoot>>,
) {
    for entity in &root_query {
        commands.entity(entity).despawn();
    }
}
