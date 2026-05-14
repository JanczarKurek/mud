//! Generic mount/unmount lifecycle for sidebar HUD panels.
//!
//! Each panel implements the [`MountablePanel`] trait, which carries the
//! per-panel constants (panel id, title, default floating size/position,
//! the `MovableWindowId` variant, the dock-state-restore kind/height)
//! and associated types for the three marker components (undock /
//! dock-back / floating-root) plus the [`PanelMountMode`] resource that
//! drives the state machine.
//!
//! The three generic systems below — [`sync_panel_floating_lifecycle`],
//! [`handle_panel_undock_click`], [`handle_panel_dock_click`] — handle
//! every panel through Rust monomorphisation. Per-panel modules
//! (`status_panel`, `equipment_panel`, `backpack_panel`) shrink to just
//! their `MountablePanel` impl + a body-builder helper; the rest of the
//! lifecycle plumbing is shared.
//!
//! ### Adding a new mountable panel
//!
//! 1. Add `MovableWindowId::<MyPanel>` and three marker components
//!    (`*UndockButton`, `*DockButton`, `*FloatingRoot`).
//! 2. Add a `<Panel>Mode(pub PanelMountMode)` resource and impl
//!    [`PanelModeAccess`] for it.
//! 3. In a new `<panel>.rs`, declare a `Panel` marker struct and impl
//!    [`MountablePanel`] for it.
//! 4. In `setup.rs`, swap the panel's `spawn_docked_panel(...)` call for
//!    `spawn_docked_panel_with_extras(...)` so the undock button gets
//!    injected into the title bar.
//! 5. Register the resource + three generic systems for the new
//!    marker in `ui::mod`.

use std::marker::PhantomData;

use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::ui::movable_window::{
    spawn_movable_window, spawn_themed_icon_button, MovableWindowDrag, MovableWindowEntities,
    MovableWindowId, MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::resources::{DockedPanel, DockedPanelKind, DockedPanelState};
use crate::ui::theme::{Palette, UiThemeAssets};

/// Whether a HUD sidebar panel is currently docked in the right sidebar
/// or floating as a `MovableWindow`. The floating variant remembers its
/// last on-screen position so re-popping out lands the window where it
/// was before the user re-docked it.
#[derive(Clone, Copy, Debug)]
pub enum PanelMountMode {
    Mounted,
    Floating { last_position: Vec2 },
}

impl Default for PanelMountMode {
    fn default() -> Self {
        Self::Mounted
    }
}

/// Trait for the per-panel mode resource so the generic systems can read
/// and mutate the mount state without knowing the concrete resource
/// type. The impl on each `<Panel>Mode(pub PanelMountMode)` newtype is a
/// one-liner.
pub trait PanelModeAccess: Resource + Default {
    fn mode(&self) -> PanelMountMode;
    fn set_mode(&mut self, mode: PanelMountMode);
}

/// Compile-time description of a mountable HUD panel. Implementors are
/// zero-sized marker structs (one per panel); the associated types and
/// constants carry the rest of the lifecycle wiring.
pub trait MountablePanel: Send + Sync + 'static {
    /// Resource holding the panel's current mode. Click handlers and
    /// the lifecycle system access it through [`PanelModeAccess`].
    type Mode: PanelModeAccess;
    /// Marker on the docked panel's title-bar undock arrow button.
    type UndockButton: Component + Default;
    /// Marker on the floating window's title-bar dock-back button.
    type DockButton: Component + Default;
    /// Marker on the floating window's root entity. The lifecycle
    /// system queries by this to find / despawn the window.
    type FloatingRoot: Component + Default;

    /// Stable id used by `DockedPanelState` for the docked variant.
    const PANEL_ID: usize;
    /// Stable id used by `MovableWindow` for the floating variant.
    const MOVABLE_WINDOW_ID: MovableWindowId;
    /// Title shown on both the docked and floating variants.
    const TITLE: &'static str;
    /// Default size for the floating window on first pop-out.
    const FLOATING_SIZE: Vec2;
    /// Default screen position for the floating window on first
    /// pop-out (used as the fallback if `last_position` isn't set yet).
    const FLOATING_POSITION: Vec2;
    /// `DockedPanelKind` used when re-pushing the docked variant after
    /// the user docks the panel back from a floating state.
    const PANEL_KIND: DockedPanelKind;
    /// Default height of the docked variant. Used by the re-push path.
    const PANEL_HEIGHT: f32;

    /// Body builder shared between docked and floating variants. The
    /// docked variant calls this from inside `spawn_docked_panel`'s body
    /// closure; the floating variant calls it from the lifecycle system
    /// when spawning the `MovableWindow`. Pass `theme` for panels whose
    /// body needs sprite handles (slot frames etc.); panels with
    /// theme-free bodies can ignore the arg.
    fn spawn_body(parent: &mut ChildSpawnerCommands, theme: &UiThemeAssets, palette: &Palette);
}

/// Reconcile the on-screen state of a mountable panel each frame
/// against its [`MountablePanel::Mode`]:
///
/// | Mode      | Docked panel        | Floating window      |
/// |-----------|---------------------|----------------------|
/// | Mounted   | Visible (re-pushed) | Despawned            |
/// | Floating  | Removed from dock   | Spawned (1 instance) |
///
/// Also handles the duplicate-suppression edge case where the user
/// reopens the docked panel through the menu bar while the floating
/// window is already up.
pub fn sync_panel_floating_lifecycle<P: MountablePanel>(
    mut commands: Commands,
    mode: Res<P::Mode>,
    mut panel_state: ResMut<DockedPanelState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    existing: Query<Entity, With<P::FloatingRoot>>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let mount = mode.mode();
    let want_float = matches!(mount, PanelMountMode::Floating { .. });
    let existing_root = existing.iter().next();

    match (want_float, existing_root) {
        (true, None) => {
            // Switch to Floating — remove the docked instance and spawn
            // the floating window.
            panel_state.close_panel(P::PANEL_ID);
            let position = match mount {
                PanelMountMode::Floating { last_position } => last_position,
                _ => P::FLOATING_POSITION,
            };
            let root = spawn_floating_window_for::<P>(&mut commands, &theme, &palette, position);
            drag.focused = Some(root);
        }
        (false, Some(root)) => {
            // Switch to Mounted — despawn the floating window, re-push
            // the docked panel to the sidebar.
            commands.entity(root).despawn();
            if drag.focused == Some(root) {
                drag.focused = None;
            }
            if drag.dragging.is_some_and(|(e, _)| e == root) {
                drag.dragging = None;
            }
            if panel_state.panel(P::PANEL_ID).is_none() {
                panel_state.panels.push(DockedPanel {
                    id: P::PANEL_ID,
                    kind: P::PANEL_KIND,
                    title: P::TITLE.to_owned(),
                    height: P::PANEL_HEIGHT,
                    closable: true,
                    resizable: true,
                    movable: true,
                });
            }
        }
        (true, Some(_)) => {
            // Stable Floating state. Suppress any docked instance the
            // menu bar may have re-pushed while the float was up.
            if panel_state.panel(P::PANEL_ID).is_some() {
                panel_state.close_panel(P::PANEL_ID);
            }
        }
        (false, None) => {}
    }
}

fn spawn_floating_window_for<P: MountablePanel>(
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
        P::MOVABLE_WINDOW_ID,
        P::TITLE,
        P::FLOATING_SIZE,
        position,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );

    commands.entity(root).insert(P::FloatingRoot::default());

    // Floating window has only the dock-back button — clicking it is
    // the dismiss action. Users wanting to fully hide the panel re-dock
    // it and then close the docked variant via its X.
    let dock_image = theme.dock_button.clone();
    commands.entity(title_bar).with_children(|bar| {
        spawn_themed_icon_button(bar, dock_image, P::DockButton::default());
    });

    let theme_owned = theme.clone();
    let palette_owned = *palette;
    commands.entity(body).with_children(|body| {
        P::spawn_body(body, &theme_owned, &palette_owned);
    });

    root
}

/// Click on the docked panel's undock arrow → flip mode to Floating.
pub fn handle_panel_undock_click<P: MountablePanel>(
    interactions: Query<&Interaction, (Changed<Interaction>, With<P::UndockButton>)>,
    mut mode: ResMut<P::Mode>,
) {
    if interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed))
    {
        if !matches!(mode.mode(), PanelMountMode::Floating { .. }) {
            mode.set_mode(PanelMountMode::Floating {
                last_position: P::FLOATING_POSITION,
            });
        }
    }
}

/// Click on the floating window's dock-back button → flip mode to Mounted.
pub fn handle_panel_dock_click<P: MountablePanel>(
    interactions: Query<&Interaction, (Changed<Interaction>, With<P::DockButton>)>,
    mut mode: ResMut<P::Mode>,
) {
    if interactions
        .iter()
        .any(|i| matches!(i, Interaction::Pressed))
    {
        mode.set_mode(PanelMountMode::Mounted);
    }
}

/// Plugin that registers everything needed for one `MountablePanel`:
/// the `Mode` resource and the three generic systems, all gated on
/// `ClientAppState::InGame`. Adding a new mountable panel to
/// `ui::mod` is one `.add_plugins(MountablePanelPlugin::<MyPanel>::default())` line.
pub struct MountablePanelPlugin<P: MountablePanel>(PhantomData<P>);

impl<P: MountablePanel> Default for MountablePanelPlugin<P> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<P: MountablePanel> Plugin for MountablePanelPlugin<P> {
    fn build(&self, app: &mut App) {
        app.init_resource::<P::Mode>().add_systems(
            Update,
            (
                handle_panel_undock_click::<P>,
                handle_panel_dock_click::<P>,
                sync_panel_floating_lifecycle::<P>
                    .after(handle_panel_undock_click::<P>)
                    .after(handle_panel_dock_click::<P>),
            )
                .run_if(in_state(ClientAppState::InGame)),
        );
    }
}
