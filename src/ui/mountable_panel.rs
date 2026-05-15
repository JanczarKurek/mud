//! Mount/unmount lifecycle for every HUD sidebar panel — singletons and
//! pooled multi-instance alike.
//!
//! The [`MountablePanel`] trait is *instance-keyed*: each impl declares a
//! [`MountablePanel::Key`] type. Singletons set `Key = ()`; pooled panels
//! (open containers, opened pouches) set `Key = usize` (the docked-pool
//! sidebar slot id). The three lifecycle systems —
//! [`handle_panel_undock_click`], [`handle_panel_dock_click`],
//! [`handle_panel_floating_close_click`], and
//! [`sync_panel_floating_lifecycle`] — are generic over `P` and iterate
//! the active key set instead of touching one global entity.
//!
//! ### Why instance-keyed for singletons too
//!
//! Pooled panels need it. Forking singletons onto their own trait meant
//! two lifecycle paths, two click-handler paths, and two floating-spawn
//! paths to keep in sync. Treating a singleton as a one-key family
//! (`Key = ()`) collapses both into one code path; the cost is one
//! `vec![()]` per active singleton per frame.
//!
//! ### Adding a new panel
//!
//! 1. Pick a `Key` (`()` for a singleton, `usize` for a pool).
//! 2. Declare four marker components implementing
//!    [`PanelInstanceMarker`]: undock button, dock button, floating
//!    root, floating close button.
//! 3. Declare a mode-store resource implementing [`ModeStore`].
//! 4. Implement [`MountablePanel`] on a zero-sized marker struct.
//! 5. Add `MountablePanelPlugin::<MyPanel>::default()` in `ui::mod`.

use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;

use bevy::prelude::*;

use crate::app::state::ClientAppState;
use crate::game::resources::PendingGameCommands;
use crate::ui::components::DockedPanelTitle;
use crate::ui::movable_window::{
    spawn_movable_window, spawn_themed_icon_button, MovableWindowDrag, MovableWindowEntities,
    MovableWindowId, MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
};
use crate::ui::resources::{DockedPanel, DockedPanelState};
use crate::ui::theme::{Palette, UiThemeAssets};

/// Whether a HUD sidebar panel is currently docked in the right sidebar
/// or floating as a `MovableWindow`.
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

/// Marker component on a per-instance UI widget (undock button, dock
/// button, floating-window root, floating close-X). The marker carries
/// the panel's instance [`Key`](MountablePanel::Key) so generic
/// handlers can look up the right mode entry without walking the
/// hierarchy. For singletons (`Key = ()`) the marker is a unit struct;
/// for pooled panels it carries a `usize` field.
pub trait PanelInstanceMarker: Component + Sized {
    type Key: Copy + Eq + Hash + Send + Sync + Debug + 'static;
    fn key(&self) -> Self::Key;
    fn new(key: Self::Key) -> Self;
}

/// Per-panel mount-state resource. Holds a [`PanelMountMode`] per
/// instance key. The lifecycle system updates entries via
/// [`set_mode`](Self::set_mode), reads them via [`mode`](Self::mode),
/// and garbage-collects entries for keys that no longer correspond to
/// an open panel via [`clear`](Self::clear).
pub trait ModeStore: Resource + Default {
    type Key: Copy + Eq + Hash + Send + Sync + Debug + 'static;
    fn mode(&self, key: Self::Key) -> PanelMountMode;
    fn set_mode(&mut self, key: Self::Key, mode: PanelMountMode);
    fn clear(&mut self, key: Self::Key);
    /// Every key the store currently holds a non-default entry for.
    /// Used by the lifecycle system to GC stale entries when their
    /// underlying panel disappears from [`DockedPanelState`].
    fn known_keys(&self) -> Vec<Self::Key>;
}

/// Compile-time wiring for a family of HUD panels. One impl per family;
/// the `Key` discriminates instances within the family.
pub trait MountablePanel: Send + Sync + 'static {
    type Key: Copy + Eq + Hash + Send + Sync + Debug + 'static;
    type Modes: ModeStore<Key = Self::Key>;
    type UndockButton: PanelInstanceMarker<Key = Self::Key>;
    type DockButton: PanelInstanceMarker<Key = Self::Key>;
    type FloatingRoot: PanelInstanceMarker<Key = Self::Key>;
    type FloatingCloseButton: PanelInstanceMarker<Key = Self::Key>;

    /// `MovableWindowId` for the floating window for this instance.
    fn movable_window_id(key: Self::Key) -> MovableWindowId;
    /// Default size of the floating window on first pop-out.
    fn floating_size(key: Self::Key) -> Vec2;
    /// Initial on-screen position used when the panel is first undocked
    /// (also the fallback when [`PanelMountMode::Floating::last_position`]
    /// isn't set yet).
    fn floating_position(key: Self::Key) -> Vec2;
    /// The `DockedPanelState` panel id this instance maps to. Singletons
    /// return a stable constant; pooled panels usually return the key
    /// itself (since `Key = panel_id`).
    fn panel_id_for(key: Self::Key) -> usize;

    /// Instance keys currently considered "open" — i.e. backed by an
    /// entry in [`DockedPanelState`]. The lifecycle system iterates
    /// only this set, so any key returned here gets reconciled and any
    /// key omitted gets cleaned up.
    fn active_keys(panel_state: &DockedPanelState) -> Vec<Self::Key>;

    /// Action when the floating window's close-X is clicked. Default:
    /// remove the panel from `DockedPanelState`. Container impl
    /// overrides to also fire `GameCommand::CloseContainer { object_id }`
    /// so the server tears down the container.
    fn handle_floating_close(
        key: Self::Key,
        panel_state: &mut DockedPanelState,
        _pending: &mut PendingGameCommands,
    ) {
        panel_state.close_panel(Self::panel_id_for(key));
    }

    /// Description of the docked variant to push into `DockedPanelState`
    /// when the user opens this panel from a menu. Returns `None` for
    /// panels that can't be menu-opened (containers — those are opened
    /// by server events). Singletons return `Some(...)` with the
    /// kind/title/height for their row.
    fn docked_definition(_key: Self::Key) -> Option<DockedPanel> {
        None
    }

    /// Build the body contents (the same one used by the docked variant
    /// at startup). Called when the floating window is spawned.
    fn spawn_body(
        parent: &mut ChildSpawnerCommands,
        key: Self::Key,
        theme: &UiThemeAssets,
        palette: &Palette,
        asset_server: &AssetServer,
    );
}

pub fn handle_panel_undock_click<P: MountablePanel>(
    interactions: Query<(&Interaction, &P::UndockButton), Changed<Interaction>>,
    mut modes: ResMut<P::Modes>,
) {
    for (interaction, button) in &interactions {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
        }
        let key = button.key();
        if !matches!(modes.mode(key), PanelMountMode::Floating { .. }) {
            modes.set_mode(
                key,
                PanelMountMode::Floating {
                    last_position: P::floating_position(key),
                },
            );
        }
    }
}

pub fn handle_panel_dock_click<P: MountablePanel>(
    interactions: Query<(&Interaction, &P::DockButton), Changed<Interaction>>,
    mut modes: ResMut<P::Modes>,
) {
    for (interaction, button) in &interactions {
        if matches!(interaction, Interaction::Pressed) {
            modes.set_mode(button.key(), PanelMountMode::Mounted);
        }
    }
}

pub fn handle_panel_floating_close_click<P: MountablePanel>(
    interactions: Query<(&Interaction, &P::FloatingCloseButton), Changed<Interaction>>,
    mut panel_state: ResMut<DockedPanelState>,
    mut pending: ResMut<PendingGameCommands>,
    mut modes: ResMut<P::Modes>,
) {
    for (interaction, button) in &interactions {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
        }
        let key = button.key();
        let panel_id = P::panel_id_for(key);
        P::handle_floating_close(key, &mut panel_state, &mut pending);
        modes.clear(key);
        // Clear the floating registry entry immediately — the lifecycle
        // pass that would have done this in `considered` is skipped
        // here because both `active_keys` and `modes.known_keys` are
        // empty for this key by the time the lifecycle runs.
        panel_state.set_floating(panel_id, false);
    }
}

/// Reconcile per-instance floating windows against [`ModeStore`] +
/// [`MountablePanel::active_keys`]:
///
/// - Key active and mode = Floating → ensure a window exists.
/// - Key active and mode = Mounted → despawn any existing window.
/// - Key no longer active → despawn window + clear mode entry.
pub fn sync_panel_floating_lifecycle<P: MountablePanel>(
    mut commands: Commands,
    mut modes: ResMut<P::Modes>,
    mut panel_state: ResMut<DockedPanelState>,
    theme: Res<UiThemeAssets>,
    palette: Res<Palette>,
    asset_server: Res<AssetServer>,
    existing: Query<(Entity, &P::FloatingRoot)>,
    mut drag: ResMut<MovableWindowDrag>,
) {
    let active: Vec<P::Key> = P::active_keys(&panel_state);

    // Despawn floating windows that shouldn't be up anymore.
    for (entity, root) in &existing {
        let key = root.key();
        let should_float =
            active.contains(&key) && matches!(modes.mode(key), PanelMountMode::Floating { .. });
        if !should_float {
            commands.entity(entity).despawn();
            if drag.focused == Some(entity) {
                drag.focused = None;
            }
            if drag.dragging.is_some_and(|(e, _)| e == entity) {
                drag.dragging = None;
            }
        }
    }

    // Spawn floating windows for Floating-mode keys without one yet.
    for key in &active {
        let key = *key;
        let PanelMountMode::Floating { last_position } = modes.mode(key) else {
            continue;
        };
        let exists = existing.iter().any(|(_, r)| r.key() == key);
        if exists {
            continue;
        }
        let root = spawn_floating_window_for::<P>(
            &mut commands,
            &theme,
            &palette,
            &asset_server,
            key,
            last_position,
        );
        drag.focused = Some(root);
    }

    // Reconcile `panel_state.floating` with our current mode entries.
    // This is the single source of truth the layout system consults
    // to decide which docked rows to hide and skip in y-stacking.
    let mut considered: Vec<P::Key> = active.clone();
    for key in modes.known_keys() {
        if !considered.contains(&key) {
            considered.push(key);
        }
    }
    for key in considered {
        let panel_id = P::panel_id_for(key);
        let floating =
            active.contains(&key) && matches!(modes.mode(key), PanelMountMode::Floating { .. });
        panel_state.set_floating(panel_id, floating);
    }

    // GC mode entries for keys no longer active.
    for key in modes.known_keys() {
        if !active.contains(&key) {
            panel_state.set_floating(P::panel_id_for(key), false);
            modes.clear(key);
        }
    }
}

fn spawn_floating_window_for<P: MountablePanel>(
    commands: &mut Commands,
    theme: &UiThemeAssets,
    palette: &Palette,
    asset_server: &AssetServer,
    key: P::Key,
    position: Vec2,
) -> Entity {
    let MovableWindowEntities {
        root,
        body,
        title_bar,
        title_text,
    } = spawn_movable_window(
        commands,
        theme,
        palette,
        P::movable_window_id(key),
        "",
        P::floating_size(key),
        position,
        MOVABLE_WINDOW_DEFAULT_MIN_SIZE,
    );

    commands
        .entity(root)
        .insert((P::FloatingRoot::new(key), crate::ui::components::HudRoot));

    // Tag the auto-spawned title text with DockedPanelTitle so the
    // existing `sync_docked_panel_titles` system keeps both docked
    // and floating labels in sync with the panel's display name.
    commands.entity(title_text).insert(DockedPanelTitle {
        panel_id: P::panel_id_for(key),
    });

    let dock_image = theme.dock_button.clone();
    let close_image = theme.close_button.clone();
    commands.entity(title_bar).with_children(|bar| {
        spawn_themed_icon_button(bar, dock_image, P::DockButton::new(key));
        spawn_themed_icon_button(bar, close_image, P::FloatingCloseButton::new(key));
    });

    let theme_owned = theme.clone();
    let palette_owned = *palette;
    let asset_server_owned = asset_server.clone();
    commands.entity(body).with_children(|body| {
        P::spawn_body(body, key, &theme_owned, &palette_owned, &asset_server_owned);
    });

    root
}

/// System set containing every [`sync_panel_floating_lifecycle`] across
/// all [`MountablePanel`] impls. The docked-panel layout system runs
/// `.after(MountablePanelLifecycleSet)` so it sees this frame's
/// `DockedPanelState::floating` updates instead of last frame's.
#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct MountablePanelLifecycleSet;

/// Plugin that registers everything for one [`MountablePanel`] impl:
/// the `Modes` resource and the four lifecycle systems, gated on
/// `ClientAppState::InGame`.
pub struct MountablePanelPlugin<P: MountablePanel>(PhantomData<P>);

impl<P: MountablePanel> Default for MountablePanelPlugin<P> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<P: MountablePanel> Plugin for MountablePanelPlugin<P> {
    fn build(&self, app: &mut App) {
        app.init_resource::<P::Modes>().add_systems(
            Update,
            (
                handle_panel_undock_click::<P>,
                handle_panel_dock_click::<P>,
                handle_panel_floating_close_click::<P>,
                sync_panel_floating_lifecycle::<P>
                    .after(handle_panel_undock_click::<P>)
                    .after(handle_panel_dock_click::<P>)
                    .after(handle_panel_floating_close_click::<P>)
                    .in_set(MountablePanelLifecycleSet),
            )
                .run_if(in_state(ClientAppState::InGame)),
        );
    }
}
