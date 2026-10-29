use super::Shared;
use rquickjs::allocator::{Allocator, RustAllocator};
use std::{ptr, sync::Arc};

/// Refuses allocations above a fixed budget and records the refusal on the
/// source, because QuickJS raises out-of-memory as an error a source can catch.
pub(super) struct BudgetAllocator {
    limit: usize,
    used: usize,
    shared: Arc<Shared>,
}
impl BudgetAllocator {
    pub(super) fn new(limit: usize, shared: Arc<Shared>) -> Self {
        Self {
            limit,
            used: 0,
            shared,
        }
    }
    fn admit(&self, size: usize) -> bool {
        let admitted = self
            .used
            .checked_add(size)
            .is_some_and(|total| total <= self.limit);
        if !admitted {
            self.shared.exhaust_memory();
        }
        admitted
    }
}
// SAFETY: every block comes from `RustAllocator`, which meets the trait
// contract. This type only counts usable sizes and returns null instead of
// delegating when a request would exceed the budget.
unsafe impl Allocator for BudgetAllocator {
    fn alloc(&mut self, size: usize) -> *mut u8 {
        if !self.admit(size) {
            return ptr::null_mut();
        }
        let block = RustAllocator.alloc(size);
        if !block.is_null() {
            // SAFETY: `block` was just returned non-null by `RustAllocator`.
            self.used += unsafe { RustAllocator::usable_size(block) };
        }
        block
    }
    fn calloc(&mut self, count: usize, size: usize) -> *mut u8 {
        let Some(total) = count.checked_mul(size) else {
            self.shared.exhaust_memory();
            return ptr::null_mut();
        };
        if !self.admit(total) {
            return ptr::null_mut();
        }
        let block = RustAllocator.calloc(count, size);
        if !block.is_null() {
            // SAFETY: `block` was just returned non-null by `RustAllocator`.
            self.used += unsafe { RustAllocator::usable_size(block) };
        }
        block
    }
    unsafe fn dealloc(&mut self, block: *mut u8) {
        // SAFETY: the caller passes a block that this allocator returned.
        unsafe {
            self.used = self.used.saturating_sub(RustAllocator::usable_size(block));
            RustAllocator.dealloc(block);
        }
    }
    unsafe fn realloc(&mut self, block: *mut u8, new_size: usize) -> *mut u8 {
        // SAFETY: the caller passes a block that this allocator returned.
        let old_size = unsafe { RustAllocator::usable_size(block) };
        if new_size > old_size && !self.admit(new_size - old_size) {
            return ptr::null_mut();
        }
        // SAFETY: the caller passes a block that this allocator returned.
        let moved = unsafe { RustAllocator.realloc(block, new_size) };
        if !moved.is_null() {
            // SAFETY: `moved` was just returned non-null by `RustAllocator`.
            let new_usable = unsafe { RustAllocator::usable_size(moved) };
            self.used = self.used.saturating_sub(old_size) + new_usable;
        }
        moved
    }
    unsafe fn usable_size(block: *mut u8) -> usize {
        // SAFETY: the caller passes a block that this allocator returned.
        unsafe { RustAllocator::usable_size(block) }
    }
}
