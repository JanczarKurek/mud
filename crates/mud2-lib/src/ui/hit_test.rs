//! Cursor-vs-UI-node hit testing. The one shared copy — this used to be
//! re-implemented per panel file.

use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;

/// Whether a logical-pixel `point` (as returned by
/// `Window::cursor_position()`) lands inside a UI node. `ComputedNode` /
/// `UiGlobalTransform` are in physical pixels, which diverge from logical on
/// HiDPI displays (e.g. scale_factor 2.0) — scale the point up before testing
/// or every click misses.
pub fn point_in_ui_node(
    point: Vec2,
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
) -> bool {
    let inv = computed.inverse_scale_factor();
    let physical_point = if inv > 0.0 { point / inv } else { point };
    computed.contains_point(*transform, physical_point)
}
