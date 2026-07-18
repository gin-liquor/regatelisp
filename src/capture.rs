//! Closure capture bookkeeping used by the lowerer. A function being
//! lowered may reference a name that lives in an enclosing function's
//! locals or captures; this module tracks, per function, which outer
//! values have already been captured and assigns each a stable
//! `CaptureSlot` in first-reference order.

use crate::ids::{CaptureSlot, LocalSlot};
use crate::ir::IrCaptureSource;

/// One function's capture list, being built up as free-variable references
/// are discovered while lowering its body. Capture slots are assigned in
/// the order names are first referenced, which is deterministic because
/// lowering always walks the Core AST left to right.
#[derive(Debug, Default)]
pub struct CaptureList {
    /// `sources[i]` is where `CaptureSlot(i)` gets its value from in the
    /// *enclosing* function's frame at `MakeClosure` time.
    sources: Vec<IrCaptureSource>,
    /// Maps an outer local slot to the capture slot already assigned to it
    /// in this function, so referencing the same outer local twice reuses
    /// one capture slot instead of allocating a duplicate.
    from_local: std::collections::HashMap<u32, CaptureSlot>,
    /// Same idea, for re-capturing a value that is itself already a
    /// capture in the immediately enclosing function.
    from_capture: std::collections::HashMap<u32, CaptureSlot>,
}

impl CaptureList {
    pub fn capture_local(&mut self, outer_slot: LocalSlot) -> CaptureSlot {
        if let Some(&slot) = self.from_local.get(&outer_slot.0) {
            return slot;
        }
        let slot = CaptureSlot(self.sources.len() as u32);
        self.sources.push(IrCaptureSource::Local(outer_slot));
        self.from_local.insert(outer_slot.0, slot);
        slot
    }

    pub fn capture_capture(&mut self, outer_slot: CaptureSlot) -> CaptureSlot {
        if let Some(&slot) = self.from_capture.get(&outer_slot.0) {
            return slot;
        }
        let slot = CaptureSlot(self.sources.len() as u32);
        self.sources.push(IrCaptureSource::Capture(outer_slot));
        self.from_capture.insert(outer_slot.0, slot);
        slot
    }

    pub fn len(&self) -> u32 {
        self.sources.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    pub fn into_sources(self) -> Vec<IrCaptureSource> {
        self.sources
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capturing_the_same_local_twice_reuses_the_slot() {
        let mut list = CaptureList::default();
        let a = list.capture_local(LocalSlot(0));
        let b = list.capture_local(LocalSlot(0));
        assert_eq!(a, b);
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn capturing_different_locals_gets_distinct_slots_in_order() {
        let mut list = CaptureList::default();
        let a = list.capture_local(LocalSlot(2));
        let b = list.capture_local(LocalSlot(5));
        assert_eq!(a, CaptureSlot(0));
        assert_eq!(b, CaptureSlot(1));
    }
}
