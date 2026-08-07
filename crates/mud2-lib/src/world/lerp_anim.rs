use bevy::math::Vec2;

/// Scalar or vector field type usable as the value of a `LinearLerp`. The
/// trait bounds are exactly what `LinearLerp::tick` and `LinearLerp::push`
/// need: copy-by-value, zero default, add for accumulation, scalar multiply
/// for the lerp, and equality so we can early-out when the residual is zero.
pub trait OffsetField:
    Copy + Default + PartialEq + std::ops::Add<Output = Self> + std::ops::Mul<f32, Output = Self>
{
}

impl OffsetField for f32 {}
impl OffsetField for Vec2 {}

/// Linear lerp toward zero, shared by the three movement-animation residuals
/// in this crate (`ViewScrollOffset`, `VisualOffset`, `FloorTransitionOffset`).
///
/// `current` is the live residual that consumers read. `start` is a snapshot
/// taken at the moment a new lerp begins; the lerp is then `start * (1 - t)`
/// over `duration` seconds. Accumulation (`push`) preserves visual continuity
/// when a new step arrives before the previous lerp has finished.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinearLerp<V: OffsetField> {
    pub current: V,
    pub start: V,
    pub elapsed: f32,
    pub duration: f32,
}

impl<V: OffsetField> LinearLerp<V> {
    /// Add `displacement` to the live residual and restart the lerp toward
    /// zero over `duration`. The visual position at the moment of the call is
    /// preserved — no snap when a new step interrupts an ongoing animation.
    pub fn push(&mut self, displacement: V, duration: f32) {
        self.current = self.current + displacement;
        self.start = self.current;
        self.elapsed = 0.0;
        self.duration = duration;
    }

    /// Advance the lerp by `dt`. Linear: hits exactly zero when
    /// `elapsed >= duration`, with no residual to hard-snap.
    pub fn tick(&mut self, dt: f32) {
        if self.duration <= 0.0 || self.current == V::default() {
            return;
        }
        self.elapsed += dt;
        let t = (self.elapsed / self.duration).clamp(0.0, 1.0);
        self.current = self.start * (1.0 - t);
        if self.elapsed >= self.duration {
            self.current = V::default();
            self.start = V::default();
            self.duration = 0.0;
        }
    }

    pub fn is_active(&self) -> bool {
        self.duration > 0.0 && self.current != V::default()
    }
}
