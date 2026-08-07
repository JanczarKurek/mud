//! Shopkeeper and Stockpile abstraction.
//!
//! `Shopkeeper` is a marker component on NPC entities flagged as merchants;
//! `Stockpile { wares }` is a sibling component on the same entity carrying
//! the wares list. The two are intentionally split: scripts (admin Python
//! REPL, restock timers, dialog side effects) can rewrite the `Stockpile`
//! without touching NPC AI, and a future content move could relocate the
//! `Stockpile` to its own entity (a "guild treasury") backing several
//! shopfronts. Keeping the two as distinct components today preserves that
//! flexibility without paying the spawn-ordering cost of a separate entity.
//!
//! YAML authoring lives in the NPC's `metadata.yaml`:
//!
//! ```yaml
//! shopkeeper:
//!   wares:
//!     - type_id: bronze_sword
//!       price_copper: 720          # 3 gold
//!       stock: infinite
//!     - type_id: leather_armor
//!       price_copper: 1200
//!       stock: 5
//! ```
//!
//! YAML authoring lives in NPC `metadata.yaml`:
//!
//! ```yaml
//! shopkeeper:
//!   wares:
//!     - type_id: bronze_sword
//!       price_copper: 720          # 3 gold
//!       stock: infinite
//!     - type_id: leather_armor
//!       price_copper: 1200
//!       stock: 5
//! ```

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Marker component: tags an NPC entity as a merchant. The actual wares list
/// lives in the sibling `Stockpile` component on the same entity. Splitting
/// these (rather than a single combined `Shopkeeper { wares }`) lets future
/// code move the `Stockpile` to a separate entity without touching the
/// shopkeeper-detection / context-menu / projection paths.
#[derive(Component, Clone, Copy, Debug)]
pub struct Shopkeeper;

/// A wares list. Lives on its own ECS entity (no `SpaceResident`/`TilePosition`
/// — invisible to the world projection). Mutable by admin scripts and
/// (Phase D) by restock timers.
#[derive(Component, Clone, Debug, Default)]
pub struct Stockpile {
    pub wares: Vec<StockEntry>,
}

#[derive(Clone, Debug)]
pub struct StockEntry {
    pub type_id: String,
    pub price_copper: u32,
    pub stock: StockMode,
}

#[derive(Clone, Copy, Debug)]
pub enum StockMode {
    Infinite,
    Finite(u32),
}

/// YAML form of `Shopkeeper`. Parsed off the NPC's `metadata.yaml` and
/// converted to an in-memory `Stockpile` at spawn time.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct ShopkeeperDef {
    #[serde(default)]
    pub wares: Vec<WareDef>,
}

/// YAML form of `StockEntry`. `stock` accepts the literal string `"infinite"`
/// or a non-negative integer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
pub struct WareDef {
    pub type_id: String,
    pub price_copper: u32,
    #[serde(default = "default_stock_mode_def")]
    pub stock: StockModeDef,
}

/// `stock:` field accepts either the literal string `"infinite"` or an
/// integer count. The `untagged` serde repr lets YAML pick the right arm
/// (numbers → `Count`, strings → `Word`).
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum StockModeDef {
    Word(StockWord),
    Count(u32),
}

impl Default for StockModeDef {
    fn default() -> Self {
        Self::Word(StockWord::Infinite)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "gen-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum StockWord {
    Infinite,
}

fn default_stock_mode_def() -> StockModeDef {
    StockModeDef::default()
}

impl WareDef {
    pub fn into_entry(self) -> StockEntry {
        let stock = match self.stock {
            StockModeDef::Word(StockWord::Infinite) => StockMode::Infinite,
            StockModeDef::Count(n) => StockMode::Finite(n),
        };
        StockEntry {
            type_id: self.type_id,
            price_copper: self.price_copper,
            stock,
        }
    }
}

impl Stockpile {
    pub fn from_def(def: &ShopkeeperDef) -> Self {
        Self::from_wares(&def.wares)
    }

    /// Build a `Stockpile` directly from a wares list. Used by the map-level
    /// vendor-stash override path: when an NPC instance carries a
    /// `vendor_stash` property naming a stash in its space, that stash's
    /// wares replace the template's defaults.
    pub fn from_wares(wares: &[WareDef]) -> Self {
        Self {
            wares: wares.iter().cloned().map(|w| w.into_entry()).collect(),
        }
    }
}

impl StockEntry {
    /// `None` for infinite stock; `Some(n)` for a finite remaining count
    /// (which may be 0 for sold-out wares).
    pub fn stock_remaining(&self) -> Option<u32> {
        match self.stock {
            StockMode::Infinite => None,
            StockMode::Finite(n) => Some(n),
        }
    }

    /// Decrement finite stock by `qty`; no-op for infinite. Returns `false`
    /// when the request would exceed available stock.
    pub fn try_take(&mut self, qty: u32) -> bool {
        match &mut self.stock {
            StockMode::Infinite => true,
            StockMode::Finite(remaining) => {
                if *remaining < qty {
                    false
                } else {
                    *remaining -= qty;
                    true
                }
            }
        }
    }
}
