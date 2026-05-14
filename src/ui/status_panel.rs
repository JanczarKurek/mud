//! Mount/unmount lifecycle for the HP/MP/XP status panel.
//!
//! The panel lives in one of two modes, tracked by [`StatusPanelMode`]:
//! - **Mounted** (default): rendered as a docked panel in the right
//!   sidebar via the existing [`DockedPanelState`] machinery.
//! - **Floating**: rendered as a [`MovableWindow`] anywhere on screen.
//!
//! Two title-bar buttons drive the transition: an undock arrow on the
//! docked panel (mode → Floating) and a dock arrow on the floating
//! window (mode → Mounted). The lifecycle system below reconciles the
//! mode each frame — spawning / despawning the floating window and
//! hiding / unhiding the docked panel as needed.
//!
//! Pilot for the broader "any HUD panel can be detached" idea. Once the
//! UX feels right here, the same triplet (mode resource + lifecycle
//! sync + two buttons) will be reused for Equipment / Backpack / etc.

use bevy::prelude::*;

use crate::ui::components::{
    StatusPanelDockButton, StatusPanelFloatingRoot, StatusPanelUndockButton,
};
use crate::ui::movable_window::{
    spawn_movable_window, spawn_themed_icon_button, MovableWindowDrag, MovableWindowEntities,
    MovableWindowId, MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::resources::{DockedPanel, DockedPanelKind, DockedPanelState, StatusPanelMode};
use crate::ui::setup::spawn_status_panel_body;
use crate::ui::theme::{Palette, UiThemeAssets};

const FLOATING_DEFAULT_SIZE: Vec2 = Vec2::new(260.0, 180.0);
const FLOATING_DEFAULT_POSITION: Vec2 = Vec2::new(360.0, 120.0);

/// Spawn a floating status window at `position`. Returns the root entity
/// (a `MovableWindow`) which carries the `StatusPanelFloatingRoot`
/// marker so the lifecycle system can find / despawn it.
fn spawn_floating_status_window(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    position: Vec2,
) -> Entity {
    let MovableWindowEntities {
        root,
        body,
        title_bar,
    } = spawn_movable_window(
        commands,
        theme,
        palette,
        MovableWindowId::StatusPanel,
        "Status",
        FLOATING_DEFAULT_SIZE,
        position,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );

    commands.entity(root).insert(StatusPanelFloatingRoot);

    // Floating window has only a dock-back button (no X). The dock
    // button IS the dismiss action — clicking it re-docks the panel into
    // the sidebar, where the regular close-X is available if the user
    // wants to fully hide it.
    let dock_image = theme.dock_button.clone();
    commands.entity(title_bar).with_children(|bar| {
        spawn_themed_icon_button(bar, dock_image, StatusPanelDockButton);
    });

    commands.entity(body).with_children(|body| {
        spawn_status_panel_body(body, palette);
    });

    root
}

/// Reconcile the on-screen state of the status panel each frame against
/// [`StatusPanelMode`]:
///
/// | Mode      | Docked panel        | Floating window      |
/// |-----------|---------------------|----------------------|
/// | Mounted   | Visible (re-pushed) | Despawned            |
/// | Floating  | Removed from dock   | Spawned (1 instance) |
pub fn sync_status_panel_floating_lifecycle(
    mut commands: Commands,
    mode: Res<StatusPanelMode>,
    mut panel_state: ResMut<DockedPanelState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    existing: Query<Entity, With<StatusPanelFloatingRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let want_float = matches!(*mode, StatusPanelMode::Floating { .. });
    let existing_root = existing.iter().next();

    match (want_float, existing_root) {
        (true, None) => {
            // Switch to Floating — remove the docked instance and spawn
            // the floating window.
            panel_state.close_panel(DockedPanelState::STATUS_PANEL_ID);
            let position = match *mode {
                StatusPanelMode::Floating { last_position } => last_position,
                _ => FLOATING_DEFAULT_POSITION,
            };
            let root = spawn_floating_status_window(&mut commands, &theme, &palette, position);
            drag.focused = Some(root);
        }
        (false, Some(root)) => {
            // Switch to Mounted — despawn the floating window, re-add
            // the docked panel to the sidebar if it isn't already there.
            commands.entity(root).despawn();
            if drag.focused == Some(root) {
                drag.focused = None;
            }
            if drag.dragging.is_some_and(|(e, _)| e == root) {
                drag.dragging = None;
            }
            if panel_state
                .panel(DockedPanelState::STATUS_PANEL_ID)
                .is_none()
            {
                panel_state.panels.push(DockedPanel {
                    id: DockedPanelState::STATUS_PANEL_ID,
                    kind: DockedPanelKind::Status,
                    title: "Status".to_owned(),
                    height: DockedPanelState::DEFAULT_STATUS_PANEL_HEIGHT,
                    closable: true,
                    resizable: true,
                    movable: true,
                });
            }
        }
        (true, Some(_)) => {
            // Stable Floating state. If the user opened the docked
            // status panel via the menu bar while it was already
            // floating, suppress the duplicate here.
            if panel_state
                .panel(DockedPanelState::STATUS_PANEL_ID)
                .is_some()
            {
                panel_state.close_panel(DockedPanelState::STATUS_PANEL_ID);
            }
        }
        (false, None) => {}
    }
}

/// Click on the docked panel's undock button → flip mode to Floating.
pub fn handle_status_panel_undock_click(
    interactions: Query<&Interaction, (Changed<Interaction>, With<StatusPanelUndockButton>)>,
    mut mode: ResMut<StatusPanelMode>,
) {
    if interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed))
    {
        if !matches!(*mode, StatusPanelMode::Floating { .. }) {
            *mode = StatusPanelMode::Floating {
                last_position: FLOATING_DEFAULT_POSITION,
            };
        }
    }
}

/// Click on the floating window's dock button → flip mode to Mounted.
pub fn handle_status_panel_dock_click(
    interactions: Query<&Interaction, (Changed<Interaction>, With<StatusPanelDockButton>)>,
    mut mode: ResMut<StatusPanelMode>,
) {
    if interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed))
    {
        *mode = StatusPanelMode::Mounted;
    }
}
