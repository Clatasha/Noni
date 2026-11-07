//! Resource retirement shared by the compositor and asynchronous encoder.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub(crate) struct Surface<T> {
    value: T,
    retired: AtomicBool,
}

impl<T> std::ops::Deref for Surface<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> Surface<T> {
    pub fn new(value: T) -> Self {
        Self {
            value,
            retired: AtomicBool::new(false),
        }
    }

    pub fn retainable(this: &Arc<Self>) -> bool {
        Arc::strong_count(this) > 1 || this.retired.load(Ordering::Acquire)
    }

    /// The pool owns the sole reference and the consumer completed normally.
    pub fn available(this: &Arc<Self>) -> bool {
        Arc::strong_count(this) == 1 && this.retired.load(Ordering::Acquire)
    }

    /// Called only under the pool lock after checking `available`.
    pub fn checkout(this: &Arc<Self>) -> Arc<Self> {
        this.retired.store(false, Ordering::Release);
        this.clone()
    }
}

impl<T: Send + Sync + 'static> Surface<T> {
    /// Invoke only after successful GPU consumption. Dropping on error keeps
    /// the allocation unretired, so it is discarded rather than overwritten.
    pub fn completion(this: &Arc<Self>) -> Box<dyn FnOnce() + Send> {
        let surface = this.clone();
        Box::new(move || surface.retired.store(true, Ordering::Release))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_gpu_copy_and_encoder_must_release_the_surface() {
        let pool = Arc::new(Surface::new(7));
        let frame = Surface::checkout(&pool);
        let compositor_copy = frame.clone();
        let encoder_completed = Surface::completion(&frame);
        drop(frame);
        assert!(!Surface::available(&pool));
        encoder_completed();
        assert!(!Surface::available(&pool), "compositor still retains it");
        drop(compositor_copy);
        assert!(Surface::available(&pool));
        let next = Surface::checkout(&pool);
        assert_eq!(**next, 7);
        drop(next);
        assert!(
            !Surface::available(&pool),
            "next checkout needs its own completion"
        );
    }

    #[test]
    fn failed_or_cancelled_encoder_never_arms_reuse() {
        let pool = Arc::new(Surface::new(7));
        let frame = Surface::checkout(&pool);
        let abandoned = Surface::completion(&frame);
        drop(frame);
        assert!(Surface::retainable(&pool));
        drop(abandoned);
        assert!(!Surface::available(&pool));
        assert!(!Surface::retainable(&pool));
    }

    #[test]
    fn unleased_probe_or_system_encoder_surface_is_single_use() {
        let pool = Arc::new(Surface::new(7));
        let frame = Surface::checkout(&pool);
        assert!(Surface::retainable(&pool));
        drop(frame);
        assert!(!Surface::available(&pool));
        assert!(!Surface::retainable(&pool));
    }
}
