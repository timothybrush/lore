// SPDX-FileCopyrightText: 2026 Epic Games, Inc.
// SPDX-License-Identifier: MIT
mod growvec;
mod rpmalloc;
mod tracking;

use std::alloc::GlobalAlloc;
use std::alloc::Layout;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

pub use growvec::GrowChunk;
pub use growvec::GrowIter;
pub use growvec::GrowIterMut;
pub use growvec::GrowVec;
pub use growvec::GrowVecMemoryStats;
pub use growvec::memory_stats;
pub use rpmalloc::RpmallocGlobalStatistics;
pub use rpmalloc::RpmallocHeapAllocator;
pub use rpmalloc::RpmallocHeapStatistics;

/// Function that allocates a block of memory with the given alignment and size.
pub type LoreAllocFn = unsafe extern "C" fn(align: usize, size: usize) -> *mut std::ffi::c_void;
/// Function that allocates a zeroed block of memory with the given alignment and size.
pub type LoreAllocZeroedFn =
    unsafe extern "C" fn(align: usize, size: usize) -> *mut std::ffi::c_void;
/// Function that resizes an existing block of memory to a new size.
pub type LoreReallocFn = unsafe extern "C" fn(
    ptr: *mut std::ffi::c_void,
    align: usize,
    size: usize,
) -> *mut std::ffi::c_void;
/// Function that frees a previously allocated block of memory.
pub type LoreDeallocFn = unsafe extern "C" fn(ptr: *mut std::ffi::c_void);

/// Set of memory functions supplied by the caller for the library to use.
pub struct ExternalAllocator {
    /// Allocation function.
    pub alloc: LoreAllocFn,
    /// Zeroed allocation function.
    pub alloc_zeroed: LoreAllocZeroedFn,
    /// Reallocation function.
    pub realloc: LoreReallocFn,
    /// Free function.
    pub dealloc: LoreDeallocFn,
}

unsafe impl GlobalAlloc for ExternalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { (self.alloc)(layout.align(), layout.size()).cast::<u8>() }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe { (self.dealloc)(ptr.cast::<std::ffi::c_void>()) };
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { (self.alloc_zeroed)(layout.align(), layout.size()).cast::<u8>() }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            (self.realloc)(ptr.cast::<std::ffi::c_void>(), layout.align(), new_size).cast::<u8>()
        }
    }
}

static EXTERN_ALLOCATOR: OnceLock<ExternalAllocator> = OnceLock::new();
static SELECTED_ALLOCATOR: OnceLock<&'static (dyn GlobalAlloc + Sync)> = OnceLock::new();
static STANDARD_ALLOCATOR: std::alloc::System = std::alloc::System;
static TRACKING_ALLOCATIONS: AtomicBool = AtomicBool::new(false);

pub fn set_external_allocator(allocator: ExternalAllocator) -> bool {
    let mut was_set = false;
    let extern_allocator = EXTERN_ALLOCATOR.get_or_init(|| {
        was_set = true;
        allocator
    });
    if was_set {
        was_set = false;
        SELECTED_ALLOCATOR.get_or_init(|| {
            was_set = true;
            extern_allocator
        });
    }
    was_set
}

fn default_allocator() -> &'static (dyn GlobalAlloc + Sync) {
    unsafe {
        let allocator = libc::getenv(c"LORE_ALLOCATOR".as_ptr().cast());
        if !allocator.is_null() && !libc::strstr(allocator, c"tracking".as_ptr().cast()).is_null() {
            TRACKING_ALLOCATIONS.store(true, Ordering::Relaxed);
        }
        if !allocator.is_null() && !libc::strstr(allocator, c"system".as_ptr().cast()).is_null() {
            &STANDARD_ALLOCATOR
        } else {
            &rpmalloc::RPMALLOC_ALLOCATOR
        }
    }
}

static RPMALLOC_HEAP_ALLOCATOR: OnceLock<RpmallocHeapAllocator> = OnceLock::new();
static GROWVEC_ALLOCATOR: OnceLock<&'static (dyn GlobalAlloc + Sync)> = OnceLock::new();

/// Returns the allocator used for `GrowVec` chunk allocations. Defaults to a
/// dedicated rpmalloc heap, falls back to System if `LORE_ALLOCATOR=system`,
/// or uses an external allocator if one was set.
pub fn growvec_allocator() -> &'static (dyn GlobalAlloc + Sync) {
    *GROWVEC_ALLOCATOR.get_or_init(|| {
        if let Some(allocator) = EXTERN_ALLOCATOR.get() {
            return allocator as &'static (dyn GlobalAlloc + Sync);
        }
        unsafe {
            let allocator = libc::getenv(c"LORE_ALLOCATOR".as_ptr().cast());
            if !allocator.is_null() && !libc::strstr(allocator, c"system".as_ptr().cast()).is_null()
            {
                &STANDARD_ALLOCATOR
            } else {
                RPMALLOC_HEAP_ALLOCATOR.get_or_init(RpmallocHeapAllocator::default)
            }
        }
    })
}

pub fn growvec_allocator_stats() -> RpmallocHeapStatistics {
    if let Some(allocator) = RPMALLOC_HEAP_ALLOCATOR.get() {
        let heap = allocator.heap.lock();
        unsafe { rpmalloc::rpmalloc_heap_statistics(*heap) }
    } else {
        RpmallocHeapStatistics::default()
    }
}

pub fn rpmalloc_global_stats() -> RpmallocGlobalStatistics {
    let mut stats = RpmallocGlobalStatistics::default();
    unsafe {
        rpmalloc::rpmalloc_global_statistics_ffi(&mut stats);
    }
    stats
}

struct LoreAllocator;

unsafe impl GlobalAlloc for LoreAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe {
            SELECTED_ALLOCATOR
                .get_or_init(|| default_allocator())
                .alloc(layout)
        };
        if TRACKING_ALLOCATIONS.load(Ordering::Relaxed) && !ptr.is_null() {
            tracking::track_alloc(ptr, layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if TRACKING_ALLOCATIONS.load(Ordering::Relaxed) && !ptr.is_null() {
            tracking::track_dealloc(ptr);
        }
        unsafe {
            SELECTED_ALLOCATOR
                .get_or_init(|| default_allocator())
                .dealloc(ptr, layout);
        }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe {
            SELECTED_ALLOCATOR
                .get_or_init(|| default_allocator())
                .alloc_zeroed(layout)
        };
        if TRACKING_ALLOCATIONS.load(Ordering::Relaxed) && !ptr.is_null() {
            tracking::track_alloc(ptr, layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe {
            SELECTED_ALLOCATOR
                .get_or_init(|| default_allocator())
                .realloc(ptr, layout, new_size)
        };
        if TRACKING_ALLOCATIONS.load(Ordering::Relaxed) {
            tracking::track_realloc(ptr, new_ptr, layout.size());
        }
        new_ptr
    }
}

pub use tracking::spawn_allocation_file_dump;

#[global_allocator]
static GLOBAL: LoreAllocator = LoreAllocator;
