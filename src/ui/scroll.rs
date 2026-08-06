//! Shared mouse-wheel scrolling core. The per-panel wheel handlers used to
//! re-implement the delta conversion, hit test, overflow gate and clamp — and
//! had drifted on their HiDPI conventions. This is the one copy; each system
//! keeps only its own query and post-scroll hook.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;

use crate::ui::hit_test::point_in_ui_node;

/// Pixels per `MouseScrollUnit::Line` step.
pub(crate) const LINE_SCROLL_PX: f32 = 21.0;

/// Wheel event → downward-positive scroll delta in pixels.
pub(crate) fn wheel_delta_y(event: &MouseWheel) -> f32 {
    let mut delta_y = -event.y;
    if event.unit == MouseScrollUnit::Line {
        delta_y *= LINE_SCROLL_PX;
    }
    delta_y
}

pub(crate) enum WheelScrollOutcome {
    /// Cursor is not over this node, or the node doesn't scroll on Y.
    Miss,
    /// Cursor hit the node but its content fits — callers `break` here, as
    /// the topmost hit node swallows the wheel even when it can't move.
    Unscrollable,
    /// The offset was clamped into range (written only when it changed).
    Scrolled,
}

/// Apply `delta_y` to a scroll node if the (logical-pixel) cursor is over it.
/// Writes `scroll` only when the clamped target differs, so Bevy change
/// detection stays quiet for at-limit spins.
pub(crate) fn apply_wheel_scroll(
    cursor_logical: Vec2,
    delta_y: f32,
    node: &Node,
    computed: &ComputedNode,
    transform: &UiGlobalTransform,
    scroll: &mut Mut<ScrollPosition>,
) -> WheelScrollOutcome {
    if !point_in_ui_node(cursor_logical, computed, transform) {
        return WheelScrollOutcome::Miss;
    }
    if node.overflow.y != bevy::ui::OverflowAxis::Scroll {
        return WheelScrollOutcome::Miss;
    }
    let max_offset =
        (computed.content_size().y - computed.size().y) * computed.inverse_scale_factor();
    if max_offset <= 0.0 {
        return WheelScrollOutcome::Unscrollable;
    }
    let target = (scroll.y + delta_y).clamp(0.0, max_offset);
    if scroll.y != target {
        scroll.y = target;
    }
    WheelScrollOutcome::Scrolled
}
