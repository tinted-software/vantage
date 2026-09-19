//! `no_std` synchronization primitives built on `lock_api`.
//!
//! `parking_lot`'s concrete types require `std` (OS blocking); instead we
//! provide minimal spin-based `RawMutex`/`RawRwLock` implementations so the
//! rest of the crate can use `lock_api`'s `Mutex`/`RwLock` wrappers with the
//! same API shape as before. Critical sections in this crate are short
//! (state updates, queue submits), so spinning is acceptable; a futex- or
//! event-based raw lock can be swapped in per-platform later without
//! touching call sites.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use lock_api::{RawMutex, RawRwLock};

/// Raw spin mutex: 0 = unlocked, 1 = locked.
pub struct RawSpinMutex {
    locked: AtomicBool,
}

impl RawSpinMutex {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }
}

impl Default for RawSpinMutex {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl RawMutex for RawSpinMutex {
    const INIT: Self = Self::new();

    type GuardMarker = lock_api::GuardNoSend;

    fn lock(&self) {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.locked.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
    }

    fn try_lock(&self) -> bool {
        self.locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    unsafe fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

/// Raw spin reader-writer lock.
///
/// Layout: bit 31 = writer-locked flag, bits 0..31 = reader count.
pub struct RawSpinRwLock {
    state: AtomicU32,
}

const WRITER_BIT: u32 = 1 << 31;
const READER_MASK: u32 = !WRITER_BIT;

impl Default for RawSpinRwLock {
    fn default() -> Self {
        Self::new()
    }
}

impl RawSpinRwLock {
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(0),
        }
    }
}

unsafe impl RawRwLock for RawSpinRwLock {
    const INIT: Self = Self::new();

    type GuardMarker = lock_api::GuardNoSend;

    fn lock_exclusive(&self) {
        // Acquire writer slot, then drain readers.
        loop {
            match self.state.compare_exchange_weak(
                0,
                WRITER_BIT,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(_) => {
                    while self.state.load(Ordering::Relaxed) != 0 {
                        core::hint::spin_loop();
                    }
                }
            }
        }
    }

    fn try_lock_exclusive(&self) -> bool {
        self.state
            .compare_exchange(0, WRITER_BIT, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    unsafe fn unlock_exclusive(&self) {
        self.state.store(0, Ordering::Release);
    }

    fn lock_shared(&self) {
        loop {
            let s = self.state.load(Ordering::Relaxed);
            if s & WRITER_BIT == 0 && s & READER_MASK != READER_MASK {
                if self
                    .state
                    .compare_exchange_weak(s, s + 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return;
                }
            } else {
                core::hint::spin_loop();
            }
        }
    }

    fn try_lock_shared(&self) -> bool {
        let s = self.state.load(Ordering::Relaxed);
        if s & WRITER_BIT == 0 && s & READER_MASK != READER_MASK {
            self.state
                .compare_exchange(s, s + 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        } else {
            false
        }
    }

    unsafe fn unlock_shared(&self) {
        self.state.fetch_sub(1, Ordering::Release);
    }
}

pub type Mutex<T> = lock_api::Mutex<RawSpinMutex, T>;
pub type RwLock<T> = lock_api::RwLock<RawSpinRwLock, T>;
