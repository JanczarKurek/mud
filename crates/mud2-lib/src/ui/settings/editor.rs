//! Editor keybindings: a sibling of [`super::model::Keybindings`] that owns
//! only the editor's discrete actions. Shares the `Binding` / `Modifiers` /
//! `SerKey` chord plumbing from `model.rs` — only the action taxonomy and
//! defaults are distinct.
//!
//! Editor bindings are namespaced separately so they never conflict with
//! gameplay bindings (e.g. plain `B` can switch to the brush in the editor
//! while `B` is bound to a different gameplay action — the two are active
//! in disjoint UI states).
//!
//! Deliberately **not** remappable: WASD/arrow camera pan (continuous, not a
//! single chord), mouse buttons, the `Escape` priority cascade, in-text-edit
//! keys (`Enter`/`Tab`/`Backspace` while editing a property), and the
//! `Shift`-click door swap (a modifier on a mouse action, not a chord).

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::input::keyboard::KeyCode;
use bevy::input::ButtonInput;
use bevy::prelude::*;

use super::model::{Binding, Bindings, Modifiers};

/// Every remappable editor action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EditorAction {
    // Tool switches.
    ToolBrush,
    ToolPortal,
    ToolFloorBrush,
    ToolSelect,
    ToolBuildingDraw,
    /// Legacy single-letter brush toggle (default `B`).
    ToolBrushLegacy,
    /// Legacy single-letter select toggle (default `M`).
    ToolSelectLegacy,
    /// `F`: cycle floor types / toggle into FloorBrush.
    ToolFloorCycle,

    // Brush.
    BrushRadiusShrink,
    BrushRadiusGrow,

    // Fill mode.
    CycleFillMode,

    // Eyedropper.
    Eyedropper,

    // Floor switching.
    FloorUp,
    FloorDown,

    /// Recent-objects strip slot (0..=8 → Alt+1..Alt+9 by default).
    SelectRecent(u8),

    // Clipboard.
    Copy,
    CopyObjectsOnly,
    Cut,
    CutObjectsOnly,
    Paste,
    Delete,

    // Paste transforms (only while paste mode is active).
    PasteRotateCw,
    PasteFlipHorizontal,
    PasteFlipVertical,

    // Undo / redo.
    Undo,
    Redo,
    RedoAlt,

    // File ops.
    Save,
    SaveAs,
    OpenMap,
}

impl EditorAction {
    pub fn label(self) -> String {
        match self {
            EditorAction::ToolBrush => "Tool: Brush".to_owned(),
            EditorAction::ToolPortal => "Tool: Portal".to_owned(),
            EditorAction::ToolFloorBrush => "Tool: Floor brush".to_owned(),
            EditorAction::ToolSelect => "Tool: Select".to_owned(),
            EditorAction::ToolBuildingDraw => "Tool: Building draw".to_owned(),
            EditorAction::ToolBrushLegacy => "Tool: Brush (single key)".to_owned(),
            EditorAction::ToolSelectLegacy => "Tool: Select (single key)".to_owned(),
            EditorAction::ToolFloorCycle => "Tool: Floor brush / cycle".to_owned(),
            EditorAction::BrushRadiusShrink => "Brush: shrink".to_owned(),
            EditorAction::BrushRadiusGrow => "Brush: grow".to_owned(),
            EditorAction::CycleFillMode => "Cycle fill mode".to_owned(),
            EditorAction::Eyedropper => "Eyedropper".to_owned(),
            EditorAction::FloorUp => "Floor: up".to_owned(),
            EditorAction::FloorDown => "Floor: down".to_owned(),
            EditorAction::SelectRecent(slot) => {
                format!("Recent object {}", slot as u16 + 1)
            }
            EditorAction::Copy => "Copy".to_owned(),
            EditorAction::CopyObjectsOnly => "Copy (objects only)".to_owned(),
            EditorAction::Cut => "Cut".to_owned(),
            EditorAction::CutObjectsOnly => "Cut (objects only)".to_owned(),
            EditorAction::Paste => "Paste".to_owned(),
            EditorAction::Delete => "Delete selection".to_owned(),
            EditorAction::PasteRotateCw => "Paste: rotate 90 CW".to_owned(),
            EditorAction::PasteFlipHorizontal => "Paste: flip horizontal".to_owned(),
            EditorAction::PasteFlipVertical => "Paste: flip vertical".to_owned(),
            EditorAction::Undo => "Undo".to_owned(),
            EditorAction::Redo => "Redo".to_owned(),
            EditorAction::RedoAlt => "Redo (alt)".to_owned(),
            EditorAction::Save => "Save map".to_owned(),
            EditorAction::SaveAs => "Save map as...".to_owned(),
            EditorAction::OpenMap => "Open map...".to_owned(),
        }
    }
}

/// Number of recent-object slots exposed as bindable actions.
pub const RECENT_OBJECT_SLOTS: u8 = 9;

/// Fixed display order for the Editor list.
pub fn all_editor_actions() -> Vec<EditorAction> {
    let mut v = vec![
        EditorAction::ToolBrush,
        EditorAction::ToolPortal,
        EditorAction::ToolFloorBrush,
        EditorAction::ToolSelect,
        EditorAction::ToolBuildingDraw,
        EditorAction::ToolBrushLegacy,
        EditorAction::ToolSelectLegacy,
        EditorAction::ToolFloorCycle,
        EditorAction::BrushRadiusShrink,
        EditorAction::BrushRadiusGrow,
        EditorAction::CycleFillMode,
        EditorAction::Eyedropper,
        EditorAction::FloorUp,
        EditorAction::FloorDown,
    ];
    for slot in 0..RECENT_OBJECT_SLOTS {
        v.push(EditorAction::SelectRecent(slot));
    }
    v.extend([
        EditorAction::Copy,
        EditorAction::CopyObjectsOnly,
        EditorAction::Cut,
        EditorAction::CutObjectsOnly,
        EditorAction::Paste,
        EditorAction::Delete,
        EditorAction::PasteRotateCw,
        EditorAction::PasteFlipHorizontal,
        EditorAction::PasteFlipVertical,
        EditorAction::Undo,
        EditorAction::Redo,
        EditorAction::RedoAlt,
        EditorAction::Save,
        EditorAction::SaveAs,
        EditorAction::OpenMap,
    ]);
    v
}

/// The remappable-binding resource for the editor. Read by every editor
/// hotkey system; mutated only by the settings UI (rebind / reset).
#[derive(Resource, Clone, Debug)]
pub struct EditorKeybindings {
    map: HashMap<EditorAction, Bindings>,
    /// Set by the UI on any change; drained by `persist_settings`.
    pub dirty: bool,
}

impl Default for EditorKeybindings {
    fn default() -> Self {
        let mut map = HashMap::new();

        // Tool switches: digits 1..5.
        map.insert(
            EditorAction::ToolBrush,
            Bindings::one(Binding::plain(KeyCode::Digit1)),
        );
        map.insert(
            EditorAction::ToolPortal,
            Bindings::one(Binding::plain(KeyCode::Digit2)),
        );
        map.insert(
            EditorAction::ToolFloorBrush,
            Bindings::one(Binding::plain(KeyCode::Digit3)),
        );
        map.insert(
            EditorAction::ToolSelect,
            Bindings::one(Binding::plain(KeyCode::Digit4)),
        );
        map.insert(
            EditorAction::ToolBuildingDraw,
            Bindings::one(Binding::plain(KeyCode::Digit5)),
        );

        // Legacy single-letter bindings.
        map.insert(
            EditorAction::ToolBrushLegacy,
            Bindings::one(Binding::plain(KeyCode::KeyB)),
        );
        map.insert(
            EditorAction::ToolSelectLegacy,
            Bindings::one(Binding::plain(KeyCode::KeyM)),
        );
        map.insert(
            EditorAction::ToolFloorCycle,
            Bindings::one(Binding::plain(KeyCode::KeyF)),
        );

        // Brush radius.
        map.insert(
            EditorAction::BrushRadiusShrink,
            Bindings::one(Binding::plain(KeyCode::BracketLeft)),
        );
        map.insert(
            EditorAction::BrushRadiusGrow,
            Bindings::one(Binding::plain(KeyCode::BracketRight)),
        );

        // Fill mode + eyedropper.
        map.insert(
            EditorAction::CycleFillMode,
            Bindings::one(Binding::plain(KeyCode::KeyG)),
        );
        map.insert(
            EditorAction::Eyedropper,
            Bindings::one(Binding::plain(KeyCode::KeyI)),
        );

        // Floor switch.
        map.insert(
            EditorAction::FloorUp,
            Bindings::one(Binding::plain(KeyCode::PageUp)),
        );
        map.insert(
            EditorAction::FloorDown,
            Bindings::one(Binding::plain(KeyCode::PageDown)),
        );

        // Recent-object slots: Alt+1 .. Alt+9.
        let recent_keys = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
        ];
        let alt = Modifiers {
            ctrl: false,
            shift: false,
            alt: true,
        };
        for (slot, key) in recent_keys.into_iter().enumerate() {
            map.insert(
                EditorAction::SelectRecent(slot as u8),
                Bindings::one(Binding::new(key, alt)),
            );
        }

        // Clipboard.
        map.insert(
            EditorAction::Copy,
            Bindings::one(Binding::ctrl(KeyCode::KeyC)),
        );
        map.insert(
            EditorAction::CopyObjectsOnly,
            Bindings::one(Binding::new(
                KeyCode::KeyC,
                Modifiers {
                    ctrl: true,
                    shift: true,
                    alt: false,
                },
            )),
        );
        map.insert(
            EditorAction::Cut,
            Bindings::one(Binding::ctrl(KeyCode::KeyX)),
        );
        map.insert(
            EditorAction::CutObjectsOnly,
            Bindings::one(Binding::new(
                KeyCode::KeyX,
                Modifiers {
                    ctrl: true,
                    shift: true,
                    alt: false,
                },
            )),
        );
        map.insert(
            EditorAction::Paste,
            Bindings::one(Binding::ctrl(KeyCode::KeyV)),
        );
        map.insert(
            EditorAction::Delete,
            Bindings::one(Binding::plain(KeyCode::Delete)),
        );

        // Paste transforms (plain keys, only active in paste mode).
        map.insert(
            EditorAction::PasteRotateCw,
            Bindings::one(Binding::plain(KeyCode::KeyR)),
        );
        map.insert(
            EditorAction::PasteFlipHorizontal,
            Bindings::one(Binding::plain(KeyCode::KeyH)),
        );
        map.insert(
            EditorAction::PasteFlipVertical,
            Bindings::one(Binding::plain(KeyCode::KeyV)),
        );

        // Undo / redo.
        map.insert(
            EditorAction::Undo,
            Bindings::one(Binding::ctrl(KeyCode::KeyZ)),
        );
        map.insert(
            EditorAction::Redo,
            Bindings::one(Binding::ctrl(KeyCode::KeyY)),
        );
        map.insert(
            EditorAction::RedoAlt,
            Bindings::one(Binding::new(
                KeyCode::KeyZ,
                Modifiers {
                    ctrl: true,
                    shift: true,
                    alt: false,
                },
            )),
        );

        // File ops.
        map.insert(
            EditorAction::Save,
            Bindings::one(Binding::ctrl(KeyCode::KeyS)),
        );
        map.insert(
            EditorAction::SaveAs,
            Bindings::one(Binding::new(
                KeyCode::KeyS,
                Modifiers {
                    ctrl: true,
                    shift: true,
                    alt: false,
                },
            )),
        );
        map.insert(
            EditorAction::OpenMap,
            Bindings::one(Binding::ctrl(KeyCode::KeyO)),
        );

        Self { map, dirty: false }
    }
}

impl EditorKeybindings {
    pub fn bindings(&self, action: EditorAction) -> Bindings {
        self.map.get(&action).copied().unwrap_or_default()
    }

    pub fn just_pressed(&self, action: EditorAction, input: &ButtonInput<KeyCode>) -> bool {
        self.bindings(action).iter().any(|b| b.just_pressed(input))
    }

    pub fn pressed(&self, action: EditorAction, input: &ButtonInput<KeyCode>) -> bool {
        self.bindings(action).iter().any(|b| b.pressed(input))
    }

    /// Replace an action's primary chord, evicting any identical chord on a
    /// different editor action. Returns the displaced action's label.
    pub fn rebind_action(&mut self, action: EditorAction, binding: Binding) -> Option<String> {
        let evicted = self.evict(binding, action);
        let entry = self.map.entry(action).or_default();
        entry.primary = Some(binding);
        self.dirty = true;
        evicted
    }

    fn evict(&mut self, binding: Binding, exclude: EditorAction) -> Option<String> {
        let mut displaced = None;
        for (action, bindings) in self.map.iter_mut() {
            if *action == exclude {
                continue;
            }
            for slot in [&mut bindings.primary, &mut bindings.secondary] {
                if slot.is_some_and(|b| b == binding) {
                    *slot = None;
                    displaced.get_or_insert_with(|| action.label());
                }
            }
        }
        displaced
    }

    pub fn reset_to_defaults(&mut self) {
        let def = EditorKeybindings::default();
        self.map = def.map;
        self.dirty = true;
    }

    /// Merge a loaded file *over* the defaults: any action absent from the
    /// file keeps its default chord, so newly-added actions always get a
    /// sensible binding without a migration.
    pub fn apply_overrides(&mut self, actions: impl IntoIterator<Item = (EditorAction, Bindings)>) {
        for (action, bindings) in actions {
            self.map.insert(action, bindings);
        }
    }

    /// Snapshot for serialization: every action's current chord, in display
    /// order.
    pub fn entries(&self) -> Vec<(EditorAction, Bindings)> {
        all_editor_actions()
            .into_iter()
            .map(|a| (a, self.bindings(a)))
            .collect()
    }
}

/// Bundles `Res<ButtonInput<KeyCode>>` + `Res<EditorKeybindings>` so editor
/// hotkey systems consume a single system param. Bevy's tuple-arity limit
/// caps function-system parameters at 16; `handle_undo_redo` already lives
/// at that ceiling and adding two distinct `Res`es would push past it.
#[derive(SystemParam)]
pub struct EditorHotkeyInput<'w> {
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    bindings: Res<'w, EditorKeybindings>,
}

impl<'w> EditorHotkeyInput<'w> {
    pub fn just_pressed(&self, action: EditorAction) -> bool {
        self.bindings.just_pressed(action, &self.keyboard)
    }

    pub fn pressed(&self, action: EditorAction) -> bool {
        self.bindings.pressed(action, &self.keyboard)
    }

    pub fn keyboard(&self) -> &ButtonInput<KeyCode> {
        &self.keyboard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_reproduces_hardcoded_bindings() {
        let kb = EditorKeybindings::default();
        assert_eq!(
            kb.bindings(EditorAction::ToolBrush).primary,
            Some(Binding::plain(KeyCode::Digit1))
        );
        assert_eq!(
            kb.bindings(EditorAction::Save).primary,
            Some(Binding::ctrl(KeyCode::KeyS))
        );
        assert_eq!(
            kb.bindings(EditorAction::SelectRecent(0)).primary,
            Some(Binding::new(
                KeyCode::Digit1,
                Modifiers {
                    ctrl: false,
                    shift: false,
                    alt: true,
                }
            ))
        );
        assert_eq!(
            kb.bindings(EditorAction::PasteRotateCw).primary,
            Some(Binding::plain(KeyCode::KeyR))
        );
        assert!(!kb.dirty);
    }

    #[test]
    fn rebind_evicts_conflicting_action() {
        let mut kb = EditorKeybindings::default();
        let displaced = kb.rebind_action(EditorAction::ToolBrush, Binding::plain(KeyCode::KeyQ));
        assert!(displaced.is_none());
        // Now bind Eyedropper to Q — must evict ToolBrush.
        let displaced = kb.rebind_action(EditorAction::Eyedropper, Binding::plain(KeyCode::KeyQ));
        assert_eq!(displaced.as_deref(), Some("Tool: Brush"));
        assert_eq!(kb.bindings(EditorAction::ToolBrush).primary, None);
    }

    #[test]
    fn ctrl_binding_requires_exact_modifier_state() {
        let kb = EditorKeybindings::default();
        let mut input = ButtonInput::<KeyCode>::default();

        // Plain S without Ctrl must NOT trigger Save.
        input.press(KeyCode::KeyS);
        assert!(!kb.just_pressed(EditorAction::Save, &input));

        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::ControlLeft);
        input.press(KeyCode::KeyS);
        assert!(kb.just_pressed(EditorAction::Save, &input));

        // Ctrl+Shift+S must trigger SaveAs, not Save.
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::ControlLeft);
        input.press(KeyCode::ShiftLeft);
        input.press(KeyCode::KeyS);
        assert!(!kb.just_pressed(EditorAction::Save, &input));
        assert!(kb.just_pressed(EditorAction::SaveAs, &input));
    }
}
