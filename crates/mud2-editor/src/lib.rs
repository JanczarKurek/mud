//! Map editor + asset/floor viewer tooling, split out of the main `mud2`
//! crate so editing gameplay code doesn't re-monomorphize the editor's UI
//! queries (and vice versa). The `mud2` binary (crates/mud2-client) plugs
//! [`editor::EditorPlugin`] into the embedded-client app via
//! `GameAppPlugin::embedded_extension`.

pub mod asset_viewer;
pub mod editor;
pub mod floor_viewer;
