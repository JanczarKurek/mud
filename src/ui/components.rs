use bevy::prelude::*;

use crate::ui::resources::{MenuAction, MenuBarId, MinimapZoom};
use crate::world::object_definitions::EquipmentSlot;

#[derive(Component)]
pub struct HealthFill;

#[derive(Component)]
pub struct ManaFill;

#[derive(Component)]
pub struct HealthLabel;

#[derive(Component)]
pub struct ManaLabel;

/// Text node that displays the active food/drink regen buff timer in the
/// status panel ("Well Fed: 0:42"). Hidden when no buff is active.
#[derive(Component)]
pub struct RegenBuffLabel;

/// Text node that displays the player's carry weight in the status panel.
/// Format: `Weight: 8.4 / 40 kg` with a trailing "(Encumbered)" tag in red
/// when the soft cap is exceeded.
#[derive(Component)]
pub struct CarryWeightLabel;

/// Marker for the XP fill bar in the status panel (mirrors `HealthFill`).
#[derive(Component)]
pub struct ExperienceFill;

/// Text node showing the player's level + XP progress
/// ("Lv 3 — 1,250 / 3,000 XP").
#[derive(Component)]
pub struct ExperienceLabel;

/// Root of the transient "Level Up!" toast overlay. The toast carries its
/// own remaining-time so the system that owns it can fade and despawn the
/// node without consulting any other state.
#[derive(Component)]
pub struct LevelUpToast {
    pub remaining_seconds: f32,
}

/// Root of the class-picker fullscreen modal that's shown to fresh
/// characters before they enter the world. Single instance — spawned by
/// `manage_class_picker` when the local player is a fresh character
/// (`!class_chosen`), despawned when the server confirms class_chosen=true.
#[derive(Component)]
pub struct ClassPickerOverlay;

/// Marker on each class option button inside the picker overlay. Click
/// dispatches `GameCommand::ChooseClass { class }`.
#[derive(Component, Clone, Copy)]
pub struct ClassPickerButton {
    pub class: crate::player::classes::Class,
}

/// Root of the post-death recap overlay. Owned by a single instance —
/// despawned when its dismiss button is clicked.
#[derive(Component)]
pub struct DeathSummaryOverlay;

/// Marker on the dismiss / continue button inside the death summary
/// overlay.
#[derive(Component)]
pub struct DeathSummaryDismissButton;

/// Floating HUD button (player-sprite icon) that toggles the Character
/// sheet modal. Sits in the top-right corner under the menu bar.
#[derive(Component)]
pub struct CharacterSheetButton;

/// Root of the Character sheet fullscreen modal. Single instance —
/// spawned/despawned in response to `CharacterSheetState.open`.
#[derive(Component)]
pub struct CharacterSheetOverlay;

/// Close-button marker inside the Character sheet modal.
#[derive(Component)]
pub struct CharacterSheetCloseButton;

#[derive(Component)]
pub struct ItemSlotButton {
    pub kind: ItemSlotKind,
}

#[derive(Component)]
pub struct ItemSlotImage {
    pub kind: ItemSlotKind,
}

#[derive(Component)]
pub struct EquipmentSlotButton;

#[derive(Component)]
pub struct ContainerSlotButton;

#[derive(Component)]
pub struct EquipmentSlotImage;

#[derive(Component)]
pub struct ContainerSlotImage;

#[derive(Component)]
pub struct DockedPanelRoot {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelCanvas;

#[derive(Component)]
pub struct DockedPanelTitle {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelDragHandle {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelCloseButton {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelBody {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DockedPanelResizeHandle {
    pub panel_id: usize,
}

#[derive(Component)]
pub struct DragPreviewRoot;

#[derive(Component)]
pub struct DragPreviewLabel;

#[derive(Component)]
pub struct DragPreviewImage;

#[derive(Component)]
pub struct DragPreviewQuantity;

#[derive(Component)]
pub struct ItemTooltipRoot;

#[derive(Component)]
pub struct ItemTooltipLabel;

#[derive(Component)]
pub struct PythonConsolePanel;

#[derive(Component)]
pub struct PythonConsoleOutput;

#[derive(Component)]
pub struct PythonConsoleInput;

#[derive(Component)]
pub struct PythonConsoleOutputViewport;

#[derive(Component)]
pub struct PythonConsoleScrollbarThumb;

#[derive(Component)]
pub struct ChatLogText;

#[derive(Component)]
pub struct ContextMenuRoot;

#[derive(Component)]
pub struct ContextMenuInspectButton;

#[derive(Component)]
pub struct ContextMenuOpenButton;

#[derive(Component)]
pub struct ContextMenuUseButton;

#[derive(Component)]
pub struct ContextMenuUseOnButton;

#[derive(Component)]
pub struct ContextMenuAttackButton;

#[derive(Component)]
pub struct ContextMenuTakePartialButton;

#[derive(Component)]
pub struct ContextMenuTalkButton;

/// Single dynamic-label button for stateful-object interactions ("Open" /
/// "Close" / "Light" / "Extinguish" / "Pull"). The label is rewritten each
/// time the menu opens against the verb chosen for the currently hovered
/// object's state.
#[derive(Component)]
pub struct ContextMenuInteractButton;

#[derive(Component)]
pub struct DialogPanelRoot;

#[derive(Component)]
pub struct DialogPanelSpeakerLabel;

#[derive(Component)]
pub struct DialogPanelBodyText;

#[derive(Component)]
pub struct DialogPanelOptionsContainer;

#[derive(Component)]
pub struct DialogPanelContinueButton;

#[derive(Component)]
pub struct DialogPanelCloseButton;

#[derive(Component)]
pub struct DialogPanelOptionButton {
    pub option_idx: usize,
}

#[derive(Component)]
pub struct CurrentTargetPanelContent;

#[derive(Component)]
pub struct ContainerPanelContent;

#[derive(Component)]
pub struct StatusPanelContent;

#[derive(Component)]
pub struct EquipmentPanelContent;

#[derive(Component)]
pub struct BackpackPanelContent;

#[derive(Component)]
pub struct CurrentCombatTargetLabel;

#[derive(Component)]
pub struct RightSidebarRoot;

#[derive(Component)]
pub struct BackpackSlotRow {
    pub row_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemSlotKind {
    Backpack(usize),
    OpenContainer {
        panel_id: usize,
        slot_index: usize,
    },
    Equipment(EquipmentSlot),
    /// A sub-slot inside a pouch panel. The owning panel is identified by
    /// `panel_id`; the panel's `DockedPanelKind::PouchInBackpack { backpack_slot }`
    /// resolves which inventory slot's `contained_slots[sub_slot_index]` to
    /// read.
    PouchInBackpack {
        panel_id: usize,
        sub_slot_index: usize,
    },
}

#[derive(Component)]
pub struct ItemSlotQuantityLabel {
    pub kind: ItemSlotKind,
}

#[derive(Component)]
pub struct TakePartialPopupRoot;

#[derive(Component)]
pub struct TakePartialDecButton;

#[derive(Component)]
pub struct TakePartialIncButton;

#[derive(Component)]
pub struct TakePartialConfirmButton;

#[derive(Component)]
pub struct TakePartialCancelButton;

#[derive(Component)]
pub struct TakePartialAmountLabel;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MinimapMode {
    HudSmall,
    FullscreenLarge,
}

#[derive(Component)]
pub struct MinimapView {
    pub mode: MinimapMode,
}

/// Holds the `Image` asset handle backing the tile window for a `MinimapView`.
/// Swapped out when zoom changes (tile span dictates image size); otherwise
/// the bytes inside are rewritten in place each frame.
#[derive(Component)]
pub struct MinimapCanvas {
    pub image_handle: Handle<Image>,
    pub last_zoom: Option<MinimapZoom>,
}

#[derive(Component)]
pub struct MinimapOverlayDot;

#[derive(Component)]
pub struct HudMinimapZoomLabel;

#[derive(Component)]
pub struct HudMinimapZoomInButton;

#[derive(Component)]
pub struct HudMinimapZoomOutButton;

#[derive(Component)]
pub struct FullMapWindowRoot;

#[derive(Component)]
pub struct FullMapZoomLabel;

#[derive(Component)]
pub struct FullMapZoomInButton;

#[derive(Component)]
pub struct FullMapZoomOutButton;

#[derive(Component)]
pub struct FullMapCloseButton;

#[derive(Component)]
pub struct FullMapBodyRoot;

#[derive(Component)]
pub struct MenuBarRoot;

#[derive(Component)]
pub struct MenuBarItemButton {
    pub menu: MenuBarId,
}

#[derive(Component)]
pub struct MenuDropdownRoot {
    pub menu: MenuBarId,
}

#[derive(Component)]
pub struct MenuDropdownEntryButton {
    pub action: MenuAction,
}
