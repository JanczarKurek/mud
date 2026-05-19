//! Keybinding data model: the `Action` taxonomy, a serializable `Binding`
//! (key + required modifiers), the per-direction `MovementBindings`, and the
//! `Keybindings` resource every input system reads through.
//!
//! `Default` reproduces the exact hardcoded layout the game shipped with, so
//! a fresh install (or a missing settings file) behaves identically to the
//! pre-settings build.
//!
//! Deliberately **not** remappable: contextual `Escape`-to-dismiss (close
//! console/chat/full-map/menu, cancel note edit) and the `F9` debug layout
//! dump. Escape-dismiss is a universal convention and making it remappable
//! risks locking the user out of a focused terminal; F9 is a dev tool.

use std::collections::HashMap;

use bevy::input::keyboard::KeyCode;
use bevy::input::ButtonInput;
use bevy::prelude::*;

use super::keycode_serde::{key_display, SerKey};

/// Number of quick-use bar slots (mirrors `ui::resources::QUICKBAR_SLOT_COUNT`,
/// kept local to avoid a cyclic-feeling dependency on the resource module).
pub const QUICKBAR_SLOTS: u8 = 10;

/// Every remappable keyboard action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Action {
    SetHome,
    RotateCcw,
    RotateCw,
    /// Use the item bound to quick-use slot `0..QUICKBAR_SLOTS`. Holding the
    /// (non-remappable) Ctrl modifier turns this into the use-on flow — the
    /// pair is one bindable unit by design.
    QuickbarUse(u8),
    ToggleBottomPanel,
    ToggleCharacterSheet,
    ToggleSkillsPanel,
    ToggleRecipeBook,
    ToggleLogWindow,
    ToggleFullMap,
    FullMapZoomIn,
    FullMapZoomOut,
    CursorUseOnToggle,
    CursorAttackToggle,
    FocusChat,
    TogglePythonConsole,
}

impl Action {
    /// Stable display label for the Controls list.
    pub fn label(self) -> String {
        match self {
            Action::SetHome => "Set respawn point".to_owned(),
            Action::RotateCcw => "Rotate object (CCW)".to_owned(),
            Action::RotateCw => "Rotate object (CW)".to_owned(),
            Action::QuickbarUse(slot) => {
                format!("Quickbar slot {} (hold Ctrl = use-on)", slot_label(slot))
            }
            Action::ToggleBottomPanel => "Toggle chat/console panel".to_owned(),
            Action::ToggleCharacterSheet => "Toggle character sheet".to_owned(),
            Action::ToggleSkillsPanel => "Toggle skills panel".to_owned(),
            Action::ToggleRecipeBook => "Toggle recipe book".to_owned(),
            Action::ToggleLogWindow => "Toggle log window".to_owned(),
            Action::ToggleFullMap => "Toggle full map".to_owned(),
            Action::FullMapZoomIn => "Full map: zoom in".to_owned(),
            Action::FullMapZoomOut => "Full map: zoom out".to_owned(),
            Action::CursorUseOnToggle => "Toggle use-on cursor".to_owned(),
            Action::CursorAttackToggle => "Toggle attack-target cursor".to_owned(),
            Action::FocusChat => "Focus chat input".to_owned(),
            Action::TogglePythonConsole => "Toggle Python console".to_owned(),
        }
    }
}

fn slot_label(slot: u8) -> String {
    // Slot 0 is bound to the "1" key, slot 9 to "0" — present 1-based with
    // slot 9 shown as "10" for clarity in the list.
    (slot as u16 + 1).to_string()
}

/// Required modifier state for a binding. Matched **exactly** against the
/// live keyboard so "no modifier" and "Ctrl required" are both expressible
/// (this reproduces the old explicit `ctrl_held` / modifier guards).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
    };
    pub const CTRL: Modifiers = Modifiers {
        ctrl: true,
        shift: false,
        alt: false,
    };

    fn from_input(input: &ButtonInput<KeyCode>) -> Modifiers {
        Modifiers {
            ctrl: input.pressed(KeyCode::ControlLeft) || input.pressed(KeyCode::ControlRight),
            shift: input.pressed(KeyCode::ShiftLeft) || input.pressed(KeyCode::ShiftRight),
            alt: input.pressed(KeyCode::AltLeft) || input.pressed(KeyCode::AltRight),
        }
    }

    fn prefix(self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s
    }
}

/// A single chord: a key plus the modifier state that must hold exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Binding {
    pub key: SerKey,
    #[serde(default)]
    pub mods: Modifiers,
}

impl Binding {
    pub fn new(key: KeyCode, mods: Modifiers) -> Self {
        Self {
            key: SerKey(key),
            mods,
        }
    }

    pub fn plain(key: KeyCode) -> Self {
        Self::new(key, Modifiers::NONE)
    }

    pub fn ctrl(key: KeyCode) -> Self {
        Self::new(key, Modifiers::CTRL)
    }

    pub fn display(&self) -> String {
        format!("{}{}", self.mods.prefix(), key_display(self.key.0))
    }

    fn matches_mods(&self, input: &ButtonInput<KeyCode>) -> bool {
        Modifiers::from_input(input) == self.mods
    }

    fn just_pressed(&self, input: &ButtonInput<KeyCode>) -> bool {
        input.just_pressed(self.key.0) && self.matches_mods(input)
    }

    fn pressed(&self, input: &ButtonInput<KeyCode>) -> bool {
        input.pressed(self.key.0) && self.matches_mods(input)
    }
}

/// Primary + optional secondary chord for one [`Action`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Bindings {
    pub primary: Option<Binding>,
    #[serde(default)]
    pub secondary: Option<Binding>,
}

impl Bindings {
    fn one(b: Binding) -> Self {
        Self {
            primary: Some(b),
            secondary: None,
        }
    }

    fn two(a: Binding, b: Binding) -> Self {
        Self {
            primary: Some(a),
            secondary: Some(b),
        }
    }

    fn iter(&self) -> impl Iterator<Item = &Binding> {
        self.primary.iter().chain(self.secondary.iter())
    }

    pub fn display(&self) -> String {
        match (self.primary, self.secondary) {
            (None, None) => "Unbound".to_owned(),
            (Some(p), None) => p.display(),
            (None, Some(s)) => s.display(),
            (Some(p), Some(s)) => format!("{} / {}", p.display(), s.display()),
        }
    }
}

/// The 8 movement directions. Each holds a list of plain (no-modifier) keys;
/// movement keeps its bespoke accumulate-and-clamp algorithm in
/// `player::systems`, so it reads these lists directly rather than going
/// through [`Action`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MovementBindings {
    pub up: Vec<SerKey>,
    pub down: Vec<SerKey>,
    pub left: Vec<SerKey>,
    pub right: Vec<SerKey>,
    pub up_left: Vec<SerKey>,
    pub up_right: Vec<SerKey>,
    pub down_left: Vec<SerKey>,
    pub down_right: Vec<SerKey>,
}

/// One movement direction, used by the UI rows and conflict scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementDir {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl MovementDir {
    pub const ALL: [MovementDir; 8] = [
        MovementDir::Up,
        MovementDir::Down,
        MovementDir::Left,
        MovementDir::Right,
        MovementDir::UpLeft,
        MovementDir::UpRight,
        MovementDir::DownLeft,
        MovementDir::DownRight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MovementDir::Up => "Move up",
            MovementDir::Down => "Move down",
            MovementDir::Left => "Move left",
            MovementDir::Right => "Move right",
            MovementDir::UpLeft => "Move up-left",
            MovementDir::UpRight => "Move up-right",
            MovementDir::DownLeft => "Move down-left",
            MovementDir::DownRight => "Move down-right",
        }
    }
}

impl MovementBindings {
    pub fn keys(&self, dir: MovementDir) -> &Vec<SerKey> {
        match dir {
            MovementDir::Up => &self.up,
            MovementDir::Down => &self.down,
            MovementDir::Left => &self.left,
            MovementDir::Right => &self.right,
            MovementDir::UpLeft => &self.up_left,
            MovementDir::UpRight => &self.up_right,
            MovementDir::DownLeft => &self.down_left,
            MovementDir::DownRight => &self.down_right,
        }
    }

    fn keys_mut(&mut self, dir: MovementDir) -> &mut Vec<SerKey> {
        match dir {
            MovementDir::Up => &mut self.up,
            MovementDir::Down => &mut self.down,
            MovementDir::Left => &mut self.left,
            MovementDir::Right => &mut self.right,
            MovementDir::UpLeft => &mut self.up_left,
            MovementDir::UpRight => &mut self.up_right,
            MovementDir::DownLeft => &mut self.down_left,
            MovementDir::DownRight => &mut self.down_right,
        }
    }

    pub fn any_pressed(&self, dir: MovementDir, input: &ButtonInput<KeyCode>) -> bool {
        self.keys(dir).iter().any(|k| input.pressed(k.0))
    }

    pub fn display(&self, dir: MovementDir) -> String {
        let keys = self.keys(dir);
        if keys.is_empty() {
            "Unbound".to_owned()
        } else {
            keys.iter()
                .map(|k| key_display(k.0))
                .collect::<Vec<_>>()
                .join(" / ")
        }
    }
}

fn ser(keys: &[KeyCode]) -> Vec<SerKey> {
    keys.iter().copied().map(SerKey).collect()
}

impl Default for MovementBindings {
    fn default() -> Self {
        Self {
            up: ser(&[KeyCode::ArrowUp, KeyCode::KeyW, KeyCode::Numpad8]),
            down: ser(&[KeyCode::ArrowDown, KeyCode::KeyS, KeyCode::Numpad2]),
            left: ser(&[KeyCode::ArrowLeft, KeyCode::KeyA, KeyCode::Numpad4]),
            right: ser(&[KeyCode::ArrowRight, KeyCode::KeyD, KeyCode::Numpad6]),
            up_left: ser(&[KeyCode::Numpad7]),
            up_right: ser(&[KeyCode::Numpad9]),
            down_left: ser(&[KeyCode::Numpad1]),
            down_right: ser(&[KeyCode::Numpad3]),
        }
    }
}

/// The remappable-binding resource. Read by every gameplay input system;
/// mutated only by the settings UI (rebind / reset).
#[derive(Resource, Clone, Debug)]
pub struct Keybindings {
    map: HashMap<Action, Bindings>,
    pub movement: MovementBindings,
    /// Set by the UI on any change; drained by `persist_settings`.
    pub dirty: bool,
}

/// Fixed display order for the Controls list (non-movement rows).
pub fn all_actions() -> Vec<Action> {
    let mut v = vec![Action::SetHome, Action::RotateCcw, Action::RotateCw];
    for slot in 0..QUICKBAR_SLOTS {
        v.push(Action::QuickbarUse(slot));
    }
    v.extend([
        Action::ToggleBottomPanel,
        Action::ToggleCharacterSheet,
        Action::ToggleSkillsPanel,
        Action::ToggleRecipeBook,
        Action::ToggleLogWindow,
        Action::ToggleFullMap,
        Action::FullMapZoomIn,
        Action::FullMapZoomOut,
        Action::CursorUseOnToggle,
        Action::CursorAttackToggle,
        Action::FocusChat,
        Action::TogglePythonConsole,
    ]);
    v
}

impl Default for Keybindings {
    fn default() -> Self {
        let mut map = HashMap::new();
        map.insert(
            Action::SetHome,
            Bindings::one(Binding::plain(KeyCode::KeyH)),
        );
        map.insert(
            Action::RotateCcw,
            Bindings::one(Binding::ctrl(KeyCode::KeyQ)),
        );
        map.insert(
            Action::RotateCw,
            Bindings::one(Binding::ctrl(KeyCode::KeyE)),
        );

        // Quickbar: slot 0 -> Digit1 .. slot 8 -> Digit9, slot 9 -> Digit0.
        let digits = [
            KeyCode::Digit1,
            KeyCode::Digit2,
            KeyCode::Digit3,
            KeyCode::Digit4,
            KeyCode::Digit5,
            KeyCode::Digit6,
            KeyCode::Digit7,
            KeyCode::Digit8,
            KeyCode::Digit9,
            KeyCode::Digit0,
        ];
        for (slot, key) in digits.into_iter().enumerate() {
            map.insert(
                Action::QuickbarUse(slot as u8),
                Bindings::one(Binding::plain(key)),
            );
        }

        map.insert(
            Action::ToggleBottomPanel,
            Bindings::one(Binding::plain(KeyCode::F1)),
        );
        map.insert(
            Action::ToggleCharacterSheet,
            Bindings::one(Binding::plain(KeyCode::KeyC)),
        );
        map.insert(
            Action::ToggleSkillsPanel,
            Bindings::one(Binding::plain(KeyCode::KeyK)),
        );
        map.insert(
            Action::ToggleRecipeBook,
            Bindings::one(Binding::plain(KeyCode::KeyR)),
        );
        map.insert(
            Action::ToggleLogWindow,
            Bindings::one(Binding::plain(KeyCode::KeyL)),
        );
        map.insert(
            Action::ToggleFullMap,
            Bindings::one(Binding::plain(KeyCode::KeyM)),
        );
        map.insert(
            Action::FullMapZoomIn,
            Bindings::two(
                Binding::plain(KeyCode::Equal),
                Binding::plain(KeyCode::NumpadAdd),
            ),
        );
        map.insert(
            Action::FullMapZoomOut,
            Bindings::two(
                Binding::plain(KeyCode::Minus),
                Binding::plain(KeyCode::NumpadSubtract),
            ),
        );
        map.insert(
            Action::CursorUseOnToggle,
            Bindings::one(Binding::plain(KeyCode::KeyU)),
        );
        map.insert(
            Action::CursorAttackToggle,
            Bindings::one(Binding::ctrl(KeyCode::KeyA)),
        );
        map.insert(
            Action::FocusChat,
            Bindings::one(Binding::plain(KeyCode::KeyT)),
        );
        map.insert(
            Action::TogglePythonConsole,
            Bindings::one(Binding::plain(KeyCode::Backquote)),
        );

        Self {
            map,
            movement: MovementBindings::default(),
            dirty: false,
        }
    }
}

impl Keybindings {
    pub fn bindings(&self, action: Action) -> Bindings {
        self.map.get(&action).copied().unwrap_or_default()
    }

    /// Edge-triggered: any bound chord whose key fired this frame with the
    /// exact required modifier state.
    pub fn just_pressed(&self, action: Action, input: &ButtonInput<KeyCode>) -> bool {
        self.bindings(action).iter().any(|b| b.just_pressed(input))
    }

    /// Held query (used by no current `Action`, but symmetrical with
    /// `just_pressed` for future "hold to X" bindings).
    pub fn pressed(&self, action: Action, input: &ButtonInput<KeyCode>) -> bool {
        self.bindings(action).iter().any(|b| b.pressed(input))
    }

    /// True if the chat-focus key fired this frame, ignoring modifiers (chat
    /// focus historically reacts to a bare keypress from the event stream).
    pub fn chat_focus_key(&self) -> Option<KeyCode> {
        self.bindings(Action::FocusChat).primary.map(|b| b.key.0)
    }

    /// Primary key bound to a quick-use slot. The quickbar treats Ctrl as an
    /// implicit "use-on" qualifier rather than part of the binding, so it
    /// needs the raw key (not a modifier-exact match).
    pub fn quickbar_key(&self, slot: u8) -> Option<KeyCode> {
        self.bindings(Action::QuickbarUse(slot))
            .primary
            .map(|b| b.key.0)
    }

    pub fn console_toggle_key(&self) -> Option<KeyCode> {
        self.bindings(Action::TogglePythonConsole)
            .primary
            .map(|b| b.key.0)
    }

    /// Replace an action's primary chord, evicting any identical chord held
    /// elsewhere. Returns the label of whatever was displaced (for the UI
    /// "was: …" hint). Marks `dirty`.
    pub fn rebind_action(&mut self, action: Action, binding: Binding) -> Option<String> {
        let evicted = self.evict(binding, ConflictExclude::Action(action));
        let entry = self.map.entry(action).or_default();
        entry.primary = Some(binding);
        self.dirty = true;
        evicted
    }

    /// Replace a movement direction with a single plain key, evicting an
    /// identical binding elsewhere. Marks `dirty`.
    pub fn rebind_movement(&mut self, dir: MovementDir, key: KeyCode) -> Option<String> {
        let binding = Binding::plain(key);
        let evicted = self.evict(binding, ConflictExclude::Movement(dir));
        *self.movement.keys_mut(dir) = vec![SerKey(key)];
        self.dirty = true;
        evicted
    }

    fn evict(&mut self, binding: Binding, exclude: ConflictExclude) -> Option<String> {
        let mut displaced = None;

        for (action, bindings) in self.map.iter_mut() {
            if matches!(exclude, ConflictExclude::Action(a) if a == *action) {
                continue;
            }
            for slot in [&mut bindings.primary, &mut bindings.secondary] {
                if slot.is_some_and(|b| b == binding) {
                    *slot = None;
                    displaced.get_or_insert_with(|| action.label());
                }
            }
        }

        // Movement directions only conflict with plain (no-mod) bindings.
        if binding.mods == Modifiers::NONE {
            for dir in MovementDir::ALL {
                if matches!(exclude, ConflictExclude::Movement(d) if d == dir) {
                    continue;
                }
                let keys = self.movement.keys_mut(dir);
                let before = keys.len();
                keys.retain(|k| k.0 != binding.key.0);
                if keys.len() != before {
                    displaced.get_or_insert_with(|| dir.label().to_owned());
                }
            }
        }

        displaced
    }

    pub fn reset_to_defaults(&mut self) {
        let def = Keybindings::default();
        self.map = def.map;
        self.movement = def.movement;
        self.dirty = true;
    }

    /// Merge a loaded file *over* the defaults: any action absent from the
    /// file keeps its default chord, so newly-added actions always get a
    /// sensible binding without a migration.
    pub fn apply_overrides(
        &mut self,
        actions: impl IntoIterator<Item = (Action, Bindings)>,
        movement: Option<MovementBindings>,
    ) {
        for (action, bindings) in actions {
            self.map.insert(action, bindings);
        }
        if let Some(movement) = movement {
            self.movement = movement;
        }
    }

    /// Snapshot for serialization: every action's current chord, in display
    /// order.
    pub fn entries(&self) -> Vec<(Action, Bindings)> {
        all_actions()
            .into_iter()
            .map(|a| (a, self.bindings(a)))
            .collect()
    }
}

#[derive(Clone, Copy)]
enum ConflictExclude {
    Action(Action),
    Movement(MovementDir),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_reproduces_hardcoded_layout() {
        let kb = Keybindings::default();
        assert_eq!(
            kb.bindings(Action::SetHome).primary,
            Some(Binding::plain(KeyCode::KeyH))
        );
        assert_eq!(
            kb.bindings(Action::RotateCw).primary,
            Some(Binding::ctrl(KeyCode::KeyE))
        );
        assert_eq!(
            kb.bindings(Action::QuickbarUse(0)).primary,
            Some(Binding::plain(KeyCode::Digit1))
        );
        assert_eq!(
            kb.bindings(Action::QuickbarUse(9)).primary,
            Some(Binding::plain(KeyCode::Digit0))
        );
        assert_eq!(
            kb.bindings(Action::FullMapZoomIn).secondary,
            Some(Binding::plain(KeyCode::NumpadAdd))
        );
        assert_eq!(kb.movement.up.len(), 3);
        assert!(!kb.dirty);
    }

    #[test]
    fn rebind_evicts_conflicting_action() {
        let mut kb = Keybindings::default();
        // Bind RotateCw to plain C, which currently toggles the char sheet.
        let displaced = kb.rebind_action(Action::RotateCw, Binding::plain(KeyCode::KeyC));
        assert_eq!(displaced.as_deref(), Some("Toggle character sheet"));
        assert_eq!(kb.bindings(Action::ToggleCharacterSheet).primary, None);
        assert_eq!(
            kb.bindings(Action::RotateCw).primary,
            Some(Binding::plain(KeyCode::KeyC))
        );
        assert!(kb.dirty);
    }

    #[test]
    fn rebind_movement_evicts_and_replaces() {
        let mut kb = Keybindings::default();
        // 'M' currently toggles the full map; rebinding "up" to M evicts it.
        let displaced = kb.rebind_movement(MovementDir::Up, KeyCode::KeyM);
        assert_eq!(displaced.as_deref(), Some("Toggle full map"));
        assert_eq!(kb.movement.up, vec![SerKey(KeyCode::KeyM)]);
        assert_eq!(kb.bindings(Action::ToggleFullMap).primary, None);
    }

    #[test]
    fn ctrl_binding_requires_exact_modifier_state() {
        let kb = Keybindings::default();
        let mut input = ButtonInput::<KeyCode>::default();

        // Plain Q with no ctrl must NOT trigger the ctrl-bound RotateCcw.
        input.press(KeyCode::KeyQ);
        assert!(!kb.just_pressed(Action::RotateCcw, &input));

        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::ControlLeft);
        input.press(KeyCode::KeyQ);
        assert!(kb.just_pressed(Action::RotateCcw, &input));

        // SetHome (plain H) must NOT fire while Ctrl is held.
        let mut input = ButtonInput::<KeyCode>::default();
        input.press(KeyCode::ControlLeft);
        input.press(KeyCode::KeyH);
        assert!(!kb.just_pressed(Action::SetHome, &input));
    }

    #[test]
    fn apply_overrides_merges_over_defaults() {
        let mut kb = Keybindings::default();
        kb.apply_overrides(
            [(
                Action::SetHome,
                Bindings::one(Binding::plain(KeyCode::KeyZ)),
            )],
            None,
        );
        // Overridden action takes the file value...
        assert_eq!(
            kb.bindings(Action::SetHome).primary,
            Some(Binding::plain(KeyCode::KeyZ))
        );
        // ...while an action absent from the override keeps its default.
        assert_eq!(
            kb.bindings(Action::ToggleFullMap).primary,
            Some(Binding::plain(KeyCode::KeyM))
        );
    }
}
