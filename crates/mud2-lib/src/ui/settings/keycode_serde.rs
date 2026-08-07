//! Stable, human-readable `KeyCode` <-> string mapping for on-disk
//! persistence.
//!
//! Bevy's `KeyCode` is not `serde`-enabled in this build (the `serialize`
//! feature is off), and even when it is, its enum representation is an
//! internal contract that can shift between versions. We own an explicit
//! string map instead: the names below are a stable contract — never rename
//! an existing arm, only add new ones.

use bevy::input::keyboard::KeyCode;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Pure modifier keys are never bindable as a primary key (they only ever
/// qualify another key via [`super::model::Modifiers`]).
pub fn is_modifier_key(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight
    )
}

macro_rules! keymap {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        /// Stable on-disk name for a bindable key, or `None` if the key is
        /// not allowed as a binding.
        pub fn keycode_to_str(key: KeyCode) -> Option<&'static str> {
            Some(match key {
                $(KeyCode::$variant => $name,)+
                _ => return None,
            })
        }

        /// Inverse of [`keycode_to_str`].
        pub fn str_to_keycode(name: &str) -> Option<KeyCode> {
            Some(match name {
                $($name => KeyCode::$variant,)+
                _ => return None,
            })
        }

        #[cfg(test)]
        fn all_mapped_keys() -> &'static [KeyCode] {
            &[$(KeyCode::$variant),+]
        }
    };
}

keymap! {
    KeyA => "A", KeyB => "B", KeyC => "C", KeyD => "D", KeyE => "E",
    KeyF => "F", KeyG => "G", KeyH => "H", KeyI => "I", KeyJ => "J",
    KeyK => "K", KeyL => "L", KeyM => "M", KeyN => "N", KeyO => "O",
    KeyP => "P", KeyQ => "Q", KeyR => "R", KeyS => "S", KeyT => "T",
    KeyU => "U", KeyV => "V", KeyW => "W", KeyX => "X", KeyY => "Y",
    KeyZ => "Z",
    Digit0 => "0", Digit1 => "1", Digit2 => "2", Digit3 => "3",
    Digit4 => "4", Digit5 => "5", Digit6 => "6", Digit7 => "7",
    Digit8 => "8", Digit9 => "9",
    Numpad0 => "Numpad0", Numpad1 => "Numpad1", Numpad2 => "Numpad2",
    Numpad3 => "Numpad3", Numpad4 => "Numpad4", Numpad5 => "Numpad5",
    Numpad6 => "Numpad6", Numpad7 => "Numpad7", Numpad8 => "Numpad8",
    Numpad9 => "Numpad9",
    NumpadAdd => "NumpadAdd", NumpadSubtract => "NumpadSubtract",
    NumpadMultiply => "NumpadMultiply", NumpadDivide => "NumpadDivide",
    NumpadEnter => "NumpadEnter", NumpadDecimal => "NumpadDecimal",
    ArrowUp => "ArrowUp", ArrowDown => "ArrowDown",
    ArrowLeft => "ArrowLeft", ArrowRight => "ArrowRight",
    F1 => "F1", F2 => "F2", F3 => "F3", F4 => "F4", F5 => "F5",
    F6 => "F6", F7 => "F7", F8 => "F8", F9 => "F9", F10 => "F10",
    F11 => "F11", F12 => "F12",
    Space => "Space", Enter => "Enter", Tab => "Tab", Escape => "Escape",
    Backspace => "Backspace", Delete => "Delete", Insert => "Insert",
    Home => "Home", End => "End", PageUp => "PageUp", PageDown => "PageDown",
    Minus => "Minus", Equal => "Equal", Backquote => "Backquote",
    BracketLeft => "BracketLeft", BracketRight => "BracketRight",
    Backslash => "Backslash", Semicolon => "Semicolon", Quote => "Quote",
    Comma => "Comma", Period => "Period", Slash => "Slash",
}

/// A `KeyCode` newtype that serializes to/from its stable string name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SerKey(pub KeyCode);

impl Serialize for SerKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match keycode_to_str(self.0) {
            Some(name) => s.serialize_str(name),
            None => Err(serde::ser::Error::custom("unbindable KeyCode")),
        }
    }
}

impl<'de> Deserialize<'de> for SerKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        str_to_keycode(&name)
            .map(SerKey)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown key name: {name}")))
    }
}

/// Short human-facing label for a key (UI display only — not the on-disk
/// name, though they coincide for most keys).
pub fn key_display(key: KeyCode) -> String {
    match key {
        KeyCode::NumpadAdd => "Num +".to_owned(),
        KeyCode::NumpadSubtract => "Num -".to_owned(),
        KeyCode::NumpadMultiply => "Num *".to_owned(),
        KeyCode::NumpadDivide => "Num /".to_owned(),
        KeyCode::NumpadEnter => "Num Enter".to_owned(),
        KeyCode::NumpadDecimal => "Num .".to_owned(),
        KeyCode::Numpad0
        | KeyCode::Numpad1
        | KeyCode::Numpad2
        | KeyCode::Numpad3
        | KeyCode::Numpad4
        | KeyCode::Numpad5
        | KeyCode::Numpad6
        | KeyCode::Numpad7
        | KeyCode::Numpad8
        | KeyCode::Numpad9 => keycode_to_str(key)
            .map(|s| s.replace("Numpad", "Num "))
            .unwrap_or_else(|| "?".to_owned()),
        KeyCode::ArrowUp => "Up".to_owned(),
        KeyCode::ArrowDown => "Down".to_owned(),
        KeyCode::ArrowLeft => "Left".to_owned(),
        KeyCode::ArrowRight => "Right".to_owned(),
        KeyCode::Backquote => "`".to_owned(),
        KeyCode::Minus => "-".to_owned(),
        KeyCode::Equal => "=".to_owned(),
        other => keycode_to_str(other).unwrap_or("?").to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_mapped_key() {
        for &key in all_mapped_keys() {
            let name = keycode_to_str(key).expect("mapped");
            assert_eq!(str_to_keycode(name), Some(key), "round trip for {name}");
        }
    }

    #[test]
    fn every_key_used_by_call_sites_is_mappable() {
        // Every literal KeyCode the gameplay systems bind today must be
        // representable so `Keybindings::default()` can be persisted.
        for key in [
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::KeyH,
            KeyCode::KeyQ,
            KeyCode::KeyE,
            KeyCode::KeyC,
            KeyCode::KeyK,
            KeyCode::KeyR,
            KeyCode::KeyL,
            KeyCode::KeyM,
            KeyCode::KeyU,
            KeyCode::KeyT,
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::Numpad1,
            KeyCode::Numpad2,
            KeyCode::Numpad3,
            KeyCode::Numpad4,
            KeyCode::Numpad6,
            KeyCode::Numpad7,
            KeyCode::Numpad8,
            KeyCode::Numpad9,
            KeyCode::Digit0,
            KeyCode::Digit1,
            KeyCode::Digit9,
            KeyCode::F1,
            KeyCode::Equal,
            KeyCode::NumpadAdd,
            KeyCode::Minus,
            KeyCode::NumpadSubtract,
            KeyCode::Backquote,
        ] {
            assert!(
                keycode_to_str(key).is_some(),
                "call-site key {key:?} must be mappable"
            );
        }
    }

    #[test]
    fn modifier_keys_are_not_bindable() {
        assert!(keycode_to_str(KeyCode::ControlLeft).is_none());
        assert!(keycode_to_str(KeyCode::ShiftRight).is_none());
        assert!(is_modifier_key(KeyCode::AltLeft));
        assert!(!is_modifier_key(KeyCode::KeyA));
    }

    #[test]
    fn serkey_serde_round_trip() {
        let k = SerKey(KeyCode::KeyQ);
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, "\"Q\"");
        let back: SerKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, k);
    }
}
