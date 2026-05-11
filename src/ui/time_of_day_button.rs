//! HUD button next to the character sheet that visualizes the in-game time
//! of day, plus a detail popup it toggles.
//!
//! Button:
//! - 48×48 "porthole" with `Overflow::clip()`.
//! - Sun and moon ride a circle whose center is at the bottom edge of the
//!   button. With `R = BUTTON_SIZE / 2` the day arc passes through both
//!   bottom corners at dawn/dusk and rises to the button center at noon,
//!   so the button is never visually empty.
//! - In spaces with `has_day_night = false` (caves, dungeons) the orbit is
//!   hidden and a static cave icon shows instead.
//!
//! Popup:
//! - A `MovableWindow` (so it's draggable, resizable, and styled like every
//!   other floating panel in the HUD). Toggled by clicking the button.
//!   Shows the in-game time (HH:MM), a larger un-clipped circular orbit, a
//!   horizon line, and a phase name + flavor description ("Dawn — First
//!   light tinges the horizon", etc.).
//!
//! All inputs (`world_time`, current space lighting) come from
//! `ClientGameState`, so this is pure presentation — no server traffic
//! involved.

use bevy::prelude::*;

use crate::game::resources::ClientGameState;
use crate::ui::menu_bar::MENU_BAR_HEIGHT;
use crate::ui::movable_window::{
    spawn_movable_window, MovableWindowDrag, MovableWindowId, MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::theme::widgets::ButtonStyle;
use crate::ui::theme::{Palette, UiThemeAssets};

const BUTTON_SIZE: f32 = 48.0;
const SPRITE_SIZE: f32 = 16.0;
/// Radius = half the button width, so dawn (angle = π/2) lands the sun
/// exactly on the bottom-right corner and dusk on the bottom-left corner.
/// With this choice the sun is *always* somewhere on or inside the visible
/// area when above the horizon — there's no "empty" zone around horizon
/// transitions like there was with a larger radius.
const ORBIT_RADIUS: f32 = BUTTON_SIZE / 2.0;
const ORBIT_CX: f32 = BUTTON_SIZE / 2.0;
const ORBIT_CY: f32 = BUTTON_SIZE;
/// Right-edge offset. The right sidebar occupies the first 272 px from the
/// screen edge, and the character-sheet button sits at `right: 294.0` in
/// the narrow gap between sidebar and corner. We tuck this button 6 px to
/// the left of the character sheet (further from the corner, still clear
/// of the sidebar): 294 + 48 + 6 = 348.
const BUTTON_RIGHT: f32 = 348.0;

/// Popup orbit canvas size (px). The popup shows the *full* circle with no
/// clipping, so both sun and moon are always visible.
const POPUP_ORBIT_CANVAS: f32 = 128.0;
const POPUP_SPRITE_SIZE: f32 = 22.0;
const POPUP_ORBIT_RADIUS: f32 = (POPUP_ORBIT_CANVAS - POPUP_SPRITE_SIZE) / 2.0 - 6.0;
const POPUP_ORBIT_CENTER: f32 = POPUP_ORBIT_CANVAS / 2.0;
const POPUP_DEFAULT_SIZE: Vec2 = Vec2::new(280.0, 320.0);

#[derive(Component)]
pub struct TimeOfDayButton;

#[derive(Component)]
pub struct TimeOfDaySun;

#[derive(Component)]
pub struct TimeOfDayMoon;

#[derive(Component)]
pub struct TimeOfDayCave;

#[derive(Resource, Default)]
pub struct TimeOfDayPopupState {
    pub open: bool,
}

#[derive(Component)]
pub struct TimeOfDayPopupRoot;

#[derive(Component)]
pub struct TimeOfDayPopupSun;

#[derive(Component)]
pub struct TimeOfDayPopupMoon;

#[derive(Component)]
pub struct TimeOfDayPopupCave;

#[derive(Component)]
pub struct TimeOfDayPopupTimeText;

#[derive(Component)]
pub struct TimeOfDayPopupPhaseText;

#[derive(Component)]
pub struct TimeOfDayPopupFlavorText;

#[derive(Component)]
pub struct TimeOfDayPopupCloseButton;

pub fn spawn_time_of_day_button(commands: &mut Commands, asset_server: &AssetServer) {
    let sun: Handle<Image> = asset_server.load("ui/hud_indicators/sun.png");
    let moon: Handle<Image> = asset_server.load("ui/hud_indicators/moon.png");
    let cave: Handle<Image> = asset_server.load("ui/hud_indicators/cave.png");

    commands
        .spawn((
            Button,
            TimeOfDayButton,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(MENU_BAR_HEIGHT + 12.0),
                right: Val::Px(BUTTON_RIGHT),
                width: Val::Px(BUTTON_SIZE),
                height: Val::Px(BUTTON_SIZE),
                border: UiRect::all(Val::Px(2.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.10, 0.08, 0.04, 0.92)),
            BorderColor::all(Color::srgb(0.60, 0.45, 0.24)),
            GlobalZIndex(50),
        ))
        .with_children(|parent| {
            let sprite_node = |left: f32, top: f32| Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(top),
                width: Val::Px(SPRITE_SIZE),
                height: Val::Px(SPRITE_SIZE),
                ..default()
            };

            // Initial: world_time defaults to 0.0 (midnight) before the
            // first replication tick, so sun is straight down (clipped)
            // and moon at the top.
            let initial_sun_x = ORBIT_CX - SPRITE_SIZE / 2.0;
            let initial_sun_y = ORBIT_CY + ORBIT_RADIUS - SPRITE_SIZE / 2.0;
            let initial_moon_x = ORBIT_CX - SPRITE_SIZE / 2.0;
            let initial_moon_y = ORBIT_CY - ORBIT_RADIUS - SPRITE_SIZE / 2.0;

            parent.spawn((
                TimeOfDaySun,
                sprite_node(initial_sun_x, initial_sun_y),
                ImageNode::new(sun),
                Visibility::Hidden,
            ));
            parent.spawn((
                TimeOfDayMoon,
                sprite_node(initial_moon_x, initial_moon_y),
                ImageNode::new(moon),
                Visibility::Inherited,
            ));
            parent.spawn((
                TimeOfDayCave,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px((BUTTON_SIZE - SPRITE_SIZE) / 2.0),
                    top: Val::Px((BUTTON_SIZE - SPRITE_SIZE) / 2.0),
                    width: Val::Px(SPRITE_SIZE),
                    height: Val::Px(SPRITE_SIZE),
                    ..default()
                },
                ImageNode::new(cave),
                Visibility::Hidden,
            ));
        });
}

fn spawn_time_of_day_window(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    asset_server: &AssetServer,
    position: Vec2,
) -> Entity {
    let sun: Handle<Image> = asset_server.load("ui/hud_indicators/sun.png");
    let moon: Handle<Image> = asset_server.load("ui/hud_indicators/moon.png");
    let cave: Handle<Image> = asset_server.load("ui/hud_indicators/cave.png");

    let spawned = spawn_movable_window(
        commands,
        theme,
        palette,
        MovableWindowId::TimeOfDay,
        "Time of Day",
        POPUP_DEFAULT_SIZE,
        position,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );

    commands.entity(spawned.root).insert(TimeOfDayPopupRoot);

    // Use our own close button so the click also flips
    // `TimeOfDayPopupState.open = false`. The shared `MovableWindow` close
    // path would despawn the window directly, leaving our state stuck on
    // `open = true`.
    commands.entity(spawned.title_bar).with_children(|bar| {
        crate::ui::setup::spawn_small_button(
            bar,
            theme,
            palette,
            ButtonStyle::Secondary,
            "X",
            TimeOfDayPopupCloseButton,
        );
    });

    commands.entity(spawned.body).with_children(|body| {
        body.spawn((
            TimeOfDayPopupTimeText,
            Text::new("--:--"),
            TextFont {
                font_size: 30.0,
                ..default()
            },
            TextColor(palette.text_accent),
            Node {
                margin: UiRect::bottom(Val::Px(4.0)),
                ..default()
            },
        ));

        body.spawn((
            Node {
                width: Val::Px(POPUP_ORBIT_CANVAS),
                height: Val::Px(POPUP_ORBIT_CANVAS),
                position_type: PositionType::Relative,
                align_self: AlignSelf::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.04, 0.08, 0.85)),
            BorderColor::all(Color::srgba(0.30, 0.26, 0.20, 1.0)),
        ))
        .with_children(|canvas| {
            // Horizon line.
            canvas.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(POPUP_ORBIT_CENTER - 0.5),
                    width: Val::Px(POPUP_ORBIT_CANVAS),
                    height: Val::Px(1.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.45, 0.38, 0.28, 0.55)),
            ));

            let sprite_node = |left: f32, top: f32| Node {
                position_type: PositionType::Absolute,
                left: Val::Px(left),
                top: Val::Px(top),
                width: Val::Px(POPUP_SPRITE_SIZE),
                height: Val::Px(POPUP_SPRITE_SIZE),
                ..default()
            };

            let cx = POPUP_ORBIT_CENTER;
            let cy = POPUP_ORBIT_CENTER;
            let half = POPUP_SPRITE_SIZE / 2.0;
            canvas.spawn((
                TimeOfDayPopupSun,
                sprite_node(cx - half, cy + POPUP_ORBIT_RADIUS - half),
                ImageNode::new(sun),
            ));
            canvas.spawn((
                TimeOfDayPopupMoon,
                sprite_node(cx - half, cy - POPUP_ORBIT_RADIUS - half),
                ImageNode::new(moon),
            ));
            canvas.spawn((
                TimeOfDayPopupCave,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(cx - half),
                    top: Val::Px(cy - half),
                    width: Val::Px(POPUP_SPRITE_SIZE),
                    height: Val::Px(POPUP_SPRITE_SIZE),
                    ..default()
                },
                ImageNode::new(cave),
                Visibility::Hidden,
            ));
        });

        body.spawn((
            TimeOfDayPopupPhaseText,
            Text::new("--"),
            TextFont {
                font_size: 18.0,
                ..default()
            },
            TextColor(palette.text_primary),
            Node {
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
        ));

        body.spawn((
            TimeOfDayPopupFlavorText,
            Text::new(""),
            TextFont {
                font_size: 13.0,
                ..default()
            },
            TextColor(palette.text_muted),
            TextLayout::new_with_justify(bevy::text::Justify::Center),
            Node {
                width: Val::Percent(100.0),
                ..default()
            },
        ));
    });

    spawned.root
}

// ─── Phase table ──────────────────────────────────────────────────────────────

struct DayPhase {
    name: &'static str,
    flavor: &'static str,
}

/// Phase boundaries on `world_time ∈ [0, 1)`. Each entry's `(threshold,
/// phase)` means "if world_time < threshold, use this phase." The list is
/// scanned in order; the final entry must have threshold > 1.0 to cover
/// the wraparound.
const PHASES: &[(f32, DayPhase)] = &[
    (
        0.21,
        DayPhase {
            name: "Night",
            flavor: "The world sleeps under cold stars.",
        },
    ),
    (
        0.29,
        DayPhase {
            name: "Dawn",
            flavor: "First light tinges the horizon.",
        },
    ),
    (
        0.46,
        DayPhase {
            name: "Morning",
            flavor: "The world stirs to life.",
        },
    ),
    (
        0.54,
        DayPhase {
            name: "Midday",
            flavor: "The sun rides high overhead.",
        },
    ),
    (
        0.71,
        DayPhase {
            name: "Afternoon",
            flavor: "Long shadows lengthen across the land.",
        },
    ),
    (
        0.79,
        DayPhase {
            name: "Dusk",
            flavor: "The west is painted gold and crimson.",
        },
    ),
    (
        1.001,
        DayPhase {
            name: "Evening",
            flavor: "Stars emerge above the rooftops.",
        },
    ),
];

fn phase_for(world_time: f32) -> &'static DayPhase {
    let t = world_time.rem_euclid(1.0);
    for (limit, phase) in PHASES {
        if t < *limit {
            return phase;
        }
    }
    // Unreachable: PHASES ends with limit > 1.0.
    &PHASES.last().unwrap().1
}

fn format_clock(world_time: f32) -> String {
    let t = world_time.rem_euclid(1.0);
    let total_minutes = (t * 24.0 * 60.0) as i32;
    let hh = (total_minutes / 60).rem_euclid(24);
    let mm = total_minutes.rem_euclid(60);
    format!("{:02}:{:02}", hh, mm)
}

// ─── Utility helpers ──────────────────────────────────────────────────────────

fn sprite_visible(center_x: f32, center_y: f32) -> bool {
    let half = SPRITE_SIZE / 2.0;
    let x_min = center_x - half;
    let x_max = center_x + half;
    let y_min = center_y - half;
    let y_max = center_y + half;
    x_max > 0.0 && x_min < BUTTON_SIZE && y_max > 0.0 && y_min < BUTTON_SIZE
}

fn set_visibility(current: &mut Visibility, desired: Visibility) {
    if *current != desired {
        *current = desired;
    }
}

fn set_px(slot: &mut Val, target: f32) {
    // Conditional write — only touch the Node if it would actually move.
    // Same discipline as `movable_window.rs::handle_movable_window_resize`.
    let new_val = Val::Px(target);
    if let Val::Px(current) = *slot {
        if (current - target).abs() < 0.5 {
            return;
        }
    }
    *slot = new_val;
}

// ─── Systems ──────────────────────────────────────────────────────────────────

pub fn update_time_of_day_indicator(
    client_state: Res<ClientGameState>,
    mut sun_q: Query<
        (&mut Node, &mut Visibility),
        (
            With<TimeOfDaySun>,
            Without<TimeOfDayMoon>,
            Without<TimeOfDayCave>,
        ),
    >,
    mut moon_q: Query<
        (&mut Node, &mut Visibility),
        (
            With<TimeOfDayMoon>,
            Without<TimeOfDaySun>,
            Without<TimeOfDayCave>,
        ),
    >,
    mut cave_q: Query<
        &mut Visibility,
        (
            With<TimeOfDayCave>,
            Without<TimeOfDaySun>,
            Without<TimeOfDayMoon>,
        ),
    >,
) {
    let Some(space) = client_state.current_space.as_ref() else {
        return;
    };

    if !space.lighting.has_day_night {
        if let Ok(mut cave_vis) = cave_q.single_mut() {
            set_visibility(&mut cave_vis, Visibility::Inherited);
        }
        if let Ok((_, mut sun_vis)) = sun_q.single_mut() {
            set_visibility(&mut sun_vis, Visibility::Hidden);
        }
        if let Ok((_, mut moon_vis)) = moon_q.single_mut() {
            set_visibility(&mut moon_vis, Visibility::Hidden);
        }
        return;
    }

    if let Ok(mut cave_vis) = cave_q.single_mut() {
        set_visibility(&mut cave_vis, Visibility::Hidden);
    }

    let angle = client_state.world_time * std::f32::consts::TAU;
    let (sin_a, cos_a) = angle.sin_cos();

    let sun_cx = ORBIT_CX + ORBIT_RADIUS * sin_a;
    let sun_cy = ORBIT_CY + ORBIT_RADIUS * cos_a;
    let moon_cx = ORBIT_CX - ORBIT_RADIUS * sin_a;
    let moon_cy = ORBIT_CY - ORBIT_RADIUS * cos_a;

    if let Ok((mut node, mut vis)) = sun_q.single_mut() {
        set_px(&mut node.left, sun_cx - SPRITE_SIZE / 2.0);
        set_px(&mut node.top, sun_cy - SPRITE_SIZE / 2.0);
        let desired = if sprite_visible(sun_cx, sun_cy) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        set_visibility(&mut vis, desired);
    }
    if let Ok((mut node, mut vis)) = moon_q.single_mut() {
        set_px(&mut node.left, moon_cx - SPRITE_SIZE / 2.0);
        set_px(&mut node.top, moon_cy - SPRITE_SIZE / 2.0);
        let desired = if sprite_visible(moon_cx, moon_cy) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        set_visibility(&mut vis, desired);
    }
}

pub fn handle_time_of_day_button_click(
    interactions: Query<&Interaction, (Changed<Interaction>, With<TimeOfDayButton>)>,
    mut state: ResMut<TimeOfDayPopupState>,
) {
    if interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed))
    {
        state.open = !state.open;
    }
}

pub fn handle_time_of_day_popup_close_click(
    interactions: Query<&Interaction, (Changed<Interaction>, With<TimeOfDayPopupCloseButton>)>,
    mut state: ResMut<TimeOfDayPopupState>,
) {
    if interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed))
    {
        state.open = false;
    }
}

/// Spawn / despawn the time-of-day window based on
/// `TimeOfDayPopupState.open`. Mirrors `sync_trade_window_lifecycle`.
pub fn sync_time_of_day_window_lifecycle(
    mut commands: Commands,
    state: Res<TimeOfDayPopupState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    asset_server: Res<AssetServer>,
    existing: Query<Entity, With<TimeOfDayPopupRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let want_open = state.open;
    let existing_root = existing.iter().next();

    match (want_open, existing_root) {
        (true, None) => {
            // First-open default anchor — under the right-side HUD. The
            // user can drag it anywhere from here.
            let position = Vec2::new(400.0, MENU_BAR_HEIGHT + 76.0);
            let root =
                spawn_time_of_day_window(&mut commands, &theme, &palette, &asset_server, position);
            drag.focused = Some(root);
        }
        (false, Some(root)) => {
            commands.entity(root).despawn();
            if drag.focused == Some(root) {
                drag.focused = None;
            }
            if drag.dragging.is_some_and(|(e, _)| e == root) {
                drag.dragging = None;
            }
        }
        (true, Some(_)) | (false, None) => {}
    }
}

#[allow(clippy::type_complexity)]
pub fn update_time_of_day_popup_contents(
    state: Res<TimeOfDayPopupState>,
    client_state: Res<ClientGameState>,
    mut sun_q: Query<
        (&mut Node, &mut Visibility),
        (
            With<TimeOfDayPopupSun>,
            Without<TimeOfDayPopupMoon>,
            Without<TimeOfDayPopupCave>,
        ),
    >,
    mut moon_q: Query<
        (&mut Node, &mut Visibility),
        (
            With<TimeOfDayPopupMoon>,
            Without<TimeOfDayPopupSun>,
            Without<TimeOfDayPopupCave>,
        ),
    >,
    mut cave_q: Query<
        &mut Visibility,
        (
            With<TimeOfDayPopupCave>,
            Without<TimeOfDayPopupSun>,
            Without<TimeOfDayPopupMoon>,
        ),
    >,
    mut time_q: Query<
        &mut Text,
        (
            With<TimeOfDayPopupTimeText>,
            Without<TimeOfDayPopupPhaseText>,
            Without<TimeOfDayPopupFlavorText>,
        ),
    >,
    mut phase_q: Query<
        &mut Text,
        (
            With<TimeOfDayPopupPhaseText>,
            Without<TimeOfDayPopupTimeText>,
            Without<TimeOfDayPopupFlavorText>,
        ),
    >,
    mut flavor_q: Query<
        &mut Text,
        (
            With<TimeOfDayPopupFlavorText>,
            Without<TimeOfDayPopupTimeText>,
            Without<TimeOfDayPopupPhaseText>,
        ),
    >,
) {
    if !state.open {
        return;
    }

    let world_time = client_state.world_time;
    let is_cave = client_state
        .current_space
        .as_ref()
        .is_some_and(|s| !s.lighting.has_day_night);

    let angle = world_time * std::f32::consts::TAU;
    let (sin_a, cos_a) = angle.sin_cos();
    let cx = POPUP_ORBIT_CENTER;
    let cy = POPUP_ORBIT_CENTER;
    let sx = cx + POPUP_ORBIT_RADIUS * sin_a - POPUP_SPRITE_SIZE / 2.0;
    let sy = cy + POPUP_ORBIT_RADIUS * cos_a - POPUP_SPRITE_SIZE / 2.0;
    let mx = cx - POPUP_ORBIT_RADIUS * sin_a - POPUP_SPRITE_SIZE / 2.0;
    let my = cy - POPUP_ORBIT_RADIUS * cos_a - POPUP_SPRITE_SIZE / 2.0;

    if let Ok((mut node, mut vis)) = sun_q.single_mut() {
        set_px(&mut node.left, sx);
        set_px(&mut node.top, sy);
        set_visibility(
            &mut vis,
            if is_cave {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            },
        );
    }
    if let Ok((mut node, mut vis)) = moon_q.single_mut() {
        set_px(&mut node.left, mx);
        set_px(&mut node.top, my);
        set_visibility(
            &mut vis,
            if is_cave {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            },
        );
    }
    if let Ok(mut vis) = cave_q.single_mut() {
        set_visibility(
            &mut vis,
            if is_cave {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        );
    }

    if let Ok(mut text) = time_q.single_mut() {
        let new_text = if is_cave {
            "--:--".to_owned()
        } else {
            format_clock(world_time)
        };
        if **text != new_text {
            **text = new_text;
        }
    }

    let (phase_name, flavor) = if is_cave {
        (
            "Underground",
            "No sky here — only stone, shadow, and the hush between heartbeats.",
        )
    } else {
        let p = phase_for(world_time);
        (p.name, p.flavor)
    };

    if let Ok(mut text) = phase_q.single_mut() {
        if **text != phase_name {
            **text = phase_name.to_owned();
        }
    }
    if let Ok(mut text) = flavor_q.single_mut() {
        if **text != flavor {
            **text = flavor.to_owned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_center_for(world_time: f32) -> (f32, f32, f32, f32) {
        let angle = world_time * std::f32::consts::TAU;
        let (s, c) = angle.sin_cos();
        let sx = ORBIT_CX + ORBIT_RADIUS * s;
        let sy = ORBIT_CY + ORBIT_RADIUS * c;
        let mx = ORBIT_CX - ORBIT_RADIUS * s;
        let my = ORBIT_CY - ORBIT_RADIUS * c;
        (sx, sy, mx, my)
    }

    #[test]
    fn sun_below_horizon_at_midnight() {
        let (sx, sy, mx, my) = body_center_for(0.0);
        assert!((sx - ORBIT_CX).abs() < 0.001);
        assert!((sy - (ORBIT_CY + ORBIT_RADIUS)).abs() < 0.001);
        assert!(!sprite_visible(sx, sy), "sun should be clipped at midnight");
        assert!(sprite_visible(mx, my), "moon should be visible at midnight");
    }

    #[test]
    fn sun_at_top_of_visible_area_at_noon() {
        let (sx, sy, mx, my) = body_center_for(0.5);
        assert!((sx - ORBIT_CX).abs() < 0.001);
        assert!((sy - (ORBIT_CY - ORBIT_RADIUS)).abs() < 0.001);
        assert!(sprite_visible(sx, sy), "sun should be visible at noon");
        assert!(!sprite_visible(mx, my), "moon should be clipped at noon");
    }

    #[test]
    fn dawn_lands_sun_on_bottom_right_corner() {
        let (sx, sy, _mx, _my) = body_center_for(0.25);
        assert!((sx - BUTTON_SIZE).abs() < 0.001);
        assert!((sy - BUTTON_SIZE).abs() < 0.001);
    }

    #[test]
    fn dusk_lands_sun_on_bottom_left_corner() {
        let (sx, sy, _mx, _my) = body_center_for(0.75);
        assert!(sx.abs() < 0.001);
        assert!((sy - BUTTON_SIZE).abs() < 0.001);
    }

    #[test]
    fn format_clock_basic_cases() {
        assert_eq!(format_clock(0.0), "00:00");
        assert_eq!(format_clock(0.25), "06:00");
        assert_eq!(format_clock(0.5), "12:00");
        assert_eq!(format_clock(0.75), "18:00");
        assert_eq!(format_clock(1.0), "00:00");
    }

    #[test]
    fn phase_covers_all_world_time() {
        for i in 0..1000 {
            let t = i as f32 / 1000.0;
            let p = phase_for(t);
            assert!(!p.name.is_empty(), "phase missing for t={}", t);
        }
    }
}
