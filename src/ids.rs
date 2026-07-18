//! Typed identifiers used throughout the IR. Each identifier category gets
//! its own newtype so the compiler catches accidental mixing (e.g. using a
//! `GlobalId` where a `LocalSlot` is expected) instead of silently letting
//! two categories of `u32` interchange.

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

define_id!(GlobalId);
define_id!(FunctionId);
define_id!(LocalSlot);
define_id!(CaptureSlot);
define_id!(LoopId);
