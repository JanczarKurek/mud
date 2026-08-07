// Split so the thin client (no `server-sim`) still compiles the two small
// config resources that ungated code reads as `Option<Res<…>>`
// (`world/setup.rs`, `app/title_screen.rs`) without pulling in the snapshot
// machinery.
pub mod config;
#[cfg(feature = "server-sim")]
pub mod snapshot;

pub use config::{WorldSaveConfig, WorldSnapshotStatus};
#[cfg(feature = "server-sim")]
pub use snapshot::*;
