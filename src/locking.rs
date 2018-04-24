use super::*;
use std::ops::{Deref, DerefMut};
use std::os::raw::c_void;

#[derive(Debug)]
///List of all possible locking mechanisms.
///Some OS implementations might not implement all of the possible lock types in this enum.
pub enum LockType {
    ///Only one reader or writer can hold this lock at once
    Mutex,
    ///Multiple readers can access the data. Writer access is exclusive.
    RwLock,
    ///No locking restrictions on the shared memory
    None,
}

#[doc(hidden)]
pub struct LockNone {}
impl MemFileLockImpl for LockNone {
    fn size_of() -> usize {0}
    fn rlock(&self, _lock_data: *mut c_void) -> Result<()> {Ok(())}
    fn wlock(&self, _lock_data: *mut c_void) -> Result<()> {Ok(())}
    fn runlock(&self, _lock_data: *mut c_void) -> () {}
    fn wunlock(&self, _lock_data: *mut c_void) -> () {}
}

///Plublic traits that custom locks need to implement
#[doc(hidden)] pub trait MemFileLockImpl {
    ///Returns the size of this lock structure that should be allocated in the shared mapping
    fn size_of() -> usize where Self: Sized;
    ///This method should only return once we have safe read access
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<()>;
    ///This method should only return once we have safe write access
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<()>;
    ///This method is automatically called when a read lock guards is dropped
    fn runlock(&self, lock_ptr: *mut c_void) -> ();
    ///This method is automatically called when a read lock guards is dropped
    fn wunlock(&self, lock_ptr: *mut c_void) -> ();
}


/* Lock Guards */

///Lock wrappping a non-mutable access to the shared data
pub struct ReadLockGuard<'a, T: 'a> {
    data: &'a T,
    lock_fn: &'a MemFileLockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T:'a> ReadLockGuard<'a, T> {
    #[doc(hidden)]
    pub fn lock(data_in: &'a T, lock_fn_in: &'a MemFileLockImpl, lock_data_in: &'a mut c_void) -> ReadLockGuard<'a, T> {
        //Acquire the read lock
        lock_fn_in.rlock(lock_data_in).unwrap();

        ReadLockGuard {
            data: data_in,
            lock_fn: lock_fn_in,
            lock_data: lock_data_in,
        }
    }
}
impl<'a, T: 'a> Drop for ReadLockGuard<'a, T> {
    fn drop(&mut self) -> () {
        self.lock_fn.runlock(self.lock_data);
    }
}
impl<'a, T> Deref for ReadLockGuard<'a, T> {
    type Target = &'a T;
    fn deref(&self) -> &Self::Target { &self.data }
}

///Lock wrappping a non-mutable access to the shared data as a slice
pub struct ReadLockGuardSlice<'a, T: 'a> {
    data: &'a [T],
    lock_fn: &'a MemFileLockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T:'a> ReadLockGuardSlice<'a, T> {
    #[doc(hidden)]
    pub fn lock(data_in: &'a [T], lock_fn_in: &'a MemFileLockImpl, lock_data_in: &'a mut c_void) -> ReadLockGuardSlice<'a, T> {
        //Acquire the read lock
        lock_fn_in.rlock(lock_data_in).unwrap();

        ReadLockGuardSlice {
            data: data_in,
            lock_fn: lock_fn_in,
            lock_data: lock_data_in,
        }
    }
}
impl<'a, T: 'a> Drop for ReadLockGuardSlice<'a, T> {
    fn drop(&mut self) -> () {
        self.lock_fn.runlock(self.lock_data);
    }
}
impl<'a, T> Deref for ReadLockGuardSlice<'a, T> {
    type Target = &'a [T];
    fn deref(&self) -> &Self::Target { &self.data }
}

///Lock wrappping a mutable access to the shared data
pub struct WriteLockGuard<'a, T: 'a> {
    data: &'a mut T,
    lock_fn: &'a MemFileLockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T:'a> WriteLockGuard<'a, T> {
    #[doc(hidden)]
    pub fn lock(data_in: &'a mut T, lock_fn_in: &'a MemFileLockImpl, lock_data_in: &'a mut c_void) -> WriteLockGuard<'a, T> {
        //Acquire the write lock
        lock_fn_in.wlock(lock_data_in).unwrap();

        WriteLockGuard {
            data: data_in,
            lock_fn: lock_fn_in,
            lock_data: lock_data_in,
        }
    }
}
impl<'a, T: 'a> Drop for WriteLockGuard<'a, T> {
    fn drop(&mut self) -> () {
        self.lock_fn.wunlock(self.lock_data);
    }
}
impl<'a, T> Deref for WriteLockGuard<'a, T> {
    type Target = &'a mut T;
    fn deref(&self) -> &Self::Target { &self.data }
}
impl<'a, T> DerefMut for WriteLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut &'a mut T {
        &mut self.data
    }
}

///Lock wrappping a mutable access to the shared data as a slice
pub struct WriteLockGuardSlice<'a, T: 'a> {
    data: &'a mut [T],
    lock_fn: &'a MemFileLockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T:'a> WriteLockGuardSlice<'a, T> {
    #[doc(hidden)]
    pub fn lock(data_in: &'a mut [T], lock_fn_in: &'a MemFileLockImpl, lock_data_in: &'a mut c_void) -> WriteLockGuardSlice<'a, T> {
        //Acquire the write lock
        lock_fn_in.wlock(lock_data_in).unwrap();

        WriteLockGuardSlice {
            data: data_in,
            lock_fn: lock_fn_in,
            lock_data: lock_data_in,
        }
    }
}
impl<'a, T: 'a> Drop for WriteLockGuardSlice<'a, T> {
    fn drop(&mut self) -> () {
        self.lock_fn.wunlock(self.lock_data);
    }
}
impl<'a, T> Deref for WriteLockGuardSlice<'a, T> {
    type Target = &'a mut [T];
    fn deref(&self) -> &Self::Target { &self.data }
}
impl<'a, T> DerefMut for WriteLockGuardSlice<'a, T> {
    fn deref_mut(&mut self) -> &mut &'a mut [T] {
        &mut self.data
    }
}
