//! A small difficulty-class builder. A `Dc` is a base value plus a list of
//! labeled modifiers, so a skill check can report both its final number and a
//! human-readable breakdown ("17 (climb 15, fatigue +2)"). The Athletics
//! traversal checks and stealth sensing assemble their DC this way so
//! situational modifiers (e.g. fatigue) are *visible* in the narration instead
//! of being silently folded into the roll.

/// A difficulty class assembled from a base plus labeled modifiers. Positive
/// modifiers make the check harder. Zero-amount modifiers are dropped so they
/// never clutter the explanation.
#[derive(Clone, Debug)]
pub struct Dc {
    base: i32,
    base_label: &'static str,
    modifiers: Vec<(i32, String)>,
}

impl Dc {
    /// Start a DC from a base value and a label for it (e.g. `"climb"`).
    pub fn new(base: i32, base_label: &'static str) -> Self {
        Self {
            base,
            base_label,
            modifiers: Vec::new(),
        }
    }

    /// Builder-style modifier add; a zero amount is ignored. Chains.
    pub fn with(mut self, amount: i32, reason: impl Into<String>) -> Self {
        self.add(amount, reason);
        self
    }

    /// Push a labeled modifier. A zero amount is ignored.
    pub fn add(&mut self, amount: i32, reason: impl Into<String>) {
        if amount != 0 {
            self.modifiers.push((amount, reason.into()));
        }
    }

    /// The resolved DC: base plus every modifier.
    pub fn total(&self) -> i32 {
        self.base + self.modifiers.iter().map(|(amount, _)| amount).sum::<i32>()
    }

    /// A human-readable breakdown. With no modifiers this is just the number
    /// ("15"); with modifiers it expands ("17 (climb 15, fatigue +2)").
    pub fn explain(&self) -> String {
        if self.modifiers.is_empty() {
            return self.total().to_string();
        }
        let mut parts = format!("{} {}", self.base_label, self.base);
        for (amount, reason) in &self.modifiers {
            parts.push_str(&format!(", {reason} {amount:+}"));
        }
        format!("{} ({})", self.total(), parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_dc_is_just_the_number() {
        let dc = Dc::new(15, "climb");
        assert_eq!(dc.total(), 15);
        assert_eq!(dc.explain(), "15");
    }

    #[test]
    fn modifiers_sum_and_explain() {
        let dc = Dc::new(15, "climb").with(2, "fatigue");
        assert_eq!(dc.total(), 17);
        assert_eq!(dc.explain(), "17 (climb 15, fatigue +2)");
    }

    #[test]
    fn zero_modifiers_are_dropped() {
        let dc = Dc::new(15, "climb").with(0, "fatigue");
        assert_eq!(dc.total(), 15);
        assert_eq!(dc.explain(), "15");
    }

    #[test]
    fn negative_modifier_shows_sign() {
        let dc = Dc::new(20, "fall").with(-3, "soft landing");
        assert_eq!(dc.total(), 17);
        assert_eq!(dc.explain(), "17 (fall 20, soft landing -3)");
    }
}
