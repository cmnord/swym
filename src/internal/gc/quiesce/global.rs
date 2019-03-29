use crate::internal::{frw_lock::FrwLock, gc::quiesce::synch_list::SynchList};
use lock_api::RawRwLock;
use std::{
    cell::UnsafeCell,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    sync::{
        atomic::{
            AtomicPtr,
            Ordering::{Acquire, Relaxed, Release},
        },
        Once,
    },
};

/// A synchronized SynchList.
///
/// The GlobalSynchList is synchronized as follows:
/// - Read access:
///     - Only a single `Synch::lock` must be held (the Synch owned by the current thread)
///     - Used for reading the current_epoch of other threads
/// - Write access:
///     - `GlobalSynchList::mutex` is locked, then all `Synch::lock`'s are acquired.
///     - Write access is only used for registering/unregistering Synchs
///
/// This "per thread/sharded" mutex locking style is optimized for reads. Reads don't cause any
/// cache line invalidation to occur for other reads.
#[repr(C)]
pub struct GlobalSynchList {
    /// The list of threads participating in the STM.
    synch_list: UnsafeCell<SynchList>,

    /// This mutex is only grabbed before modifying to the GlobalSynchList, and still requires
    /// every threads `Synch::lock` to be acquired before any mutations.
    mutex: FrwLock,
}

// GlobalSynchList is synchronized by an internal sharded lock.
unsafe impl Sync for GlobalSynchList {}

// Once allocated the SINGLETON is never deallocated.
static SINGLETON: AtomicPtr<GlobalSynchList> = AtomicPtr::new(0 as _);

impl GlobalSynchList {
    // slow path
    #[inline(never)]
    #[cold]
    fn init() -> &'static Self {
        // Once handles two threads racing to initialize the GlobalSynchList
        static INIT_QUIESCE_LIST: Once = Once::new();

        #[inline(never)]
        #[cold]
        fn do_init() {
            SINGLETON.store(
                Box::into_raw(Box::new(GlobalSynchList {
                    synch_list: UnsafeCell::new(SynchList::new()),
                    mutex:      RawRwLock::INIT,
                })),
                Release,
            );
        }

        INIT_QUIESCE_LIST.call_once(do_init);

        Self::instance()
    }

    /// Returns a references to the global thread list.
    #[inline]
    pub fn instance() -> &'static Self {
        let raw = SINGLETON.load(Acquire);
        if likely!(!raw.is_null()) {
            // SINGLETON is never freed, so once initialized, it is always valid
            unsafe { &*raw }
        } else {
            GlobalSynchList::init()
        }
    }

    /// Returns a references to the global thread list. If `instance` has been called, then
    /// instance_unchecked is safe to call.
    #[inline]
    pub unsafe fn instance_unchecked() -> &'static Self {
        let raw = SINGLETON.load(Relaxed);
        debug_assert!(
            !raw.is_null(),
            "`GlobalSynchList::instance_unchecked` called before instance was created"
        );
        &*raw
    }

    /// Unsafe without holding atleast one of the locks in the GlobalSynchList.
    #[inline]
    pub(super) unsafe fn raw(&self) -> &SynchList {
        &*self.synch_list.get()
    }

    /// Unsafe without holding all of the locks in the GlobalSynchList.
    #[inline]
    unsafe fn raw_mut(&self) -> &mut SynchList {
        &mut *self.synch_list.get()
    }

    /// Gets write access to the GlobalSynchList.
    #[inline]
    pub fn write<'a>(&'a self) -> Write<'a> {
        Write::new(self)
    }
}

/// A write guard for the GlobalSynchList.
pub struct Write<'a> {
    list: &'a GlobalSynchList,
}

impl<'a> Write<'a> {
    #[inline]
    fn new(list: &'a GlobalSynchList) -> Self {
        // Atleast one mutex has to be held in order to call `raw` safely.
        // The outer mutex is used for this purpose, and so that, under contention, two writers will
        // never deadlock.
        list.mutex.lock_exclusive();
        let list = ManuallyDrop::new(Write { list });
        // lock all the Synchs to prevent them from creating a FreezeList
        for synch in list.iter() {
            synch.lock();
        }
        ManuallyDrop::into_inner(list)
    }
}

impl<'a> Drop for Write<'a> {
    #[inline]
    fn drop(&mut self) {
        for synch in self.iter() {
            synch.unlock();
        }
        self.list.mutex.unlock_exclusive();
    }
}

impl<'a> Deref for Write<'a> {
    type Target = SynchList;

    #[inline]
    fn deref(&self) -> &SynchList {
        // we own all the sharded locks, giving us mutable access
        unsafe { self.list.raw() }
    }
}

impl<'a> DerefMut for Write<'a> {
    #[inline]
    fn deref_mut(&mut self) -> &mut SynchList {
        // we own all the sharded locks, giving us mutable access
        unsafe { self.list.raw_mut() }
    }
}
