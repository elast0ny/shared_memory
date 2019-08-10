//This file provides definitions related to locking in my_shmem.

//If you wish to implement your own lock type:
//  1. add a field to the LockType enum bellow
//  2. Go into your OS specific OS.rs and create a new pub struct
//  3. Implement the SharedMemLockImpl trait for your new struct
//  4. Make sure that your os_impl::open() and os_impl::create() initialize the lock properly in non-raw mode
use ::enum_primitive::*;

use std::ops::{Deref, DerefMut};
use std::os::raw::c_void;

use crate::{SharedMemCast, SharedMemError};

#[doc(hidden)]
pub struct GenericLock {
    /* Fields shared in the memory mapping */
    pub uid: u8,
    pub offset: usize,
    pub length: usize,
    pub data_ptr: *mut c_void,
    pub lock_ptr: *mut c_void,
    pub interface: &'static LockImpl,
}

enum_from_primitive! {
    #[derive(Debug,Copy,Clone)]
    ///List of available locking mechanisms on your platform.
    pub enum LockType {
        ///Only one reader or writer can hold this lock at once
        Mutex = 0,
        ///Multiple readers can access the data. Writer access is exclusive.
        RwLock,
    }
}

#[doc(hidden)]
pub trait LockImpl {
    ///Returns the size of the lock structure that will live in shared memory
    fn size_of(&self) -> usize;
    ///Initializes the lock
    fn init(&self, lock_info: &mut GenericLock, create_new: bool) -> Result<(), SharedMemError>;
    ///De-initializes the lock
    fn destroy(&self, lock_info: &mut GenericLock);
    ///This method should only return once we have safe read access
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<(), SharedMemError>;
    ///This method should only return once we have safe write access
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<(), SharedMemError>;
    ///This method is automatically called when a read lock guards is dropped
    fn runlock(&self, lock_ptr: *mut c_void) -> ();
    ///This method is automatically called when a read lock guards is dropped
    fn wunlock(&self, lock_ptr: *mut c_void) -> ();
}

///Provides rlock/rlock_as_slice functionnalities
pub trait ReadLockable {
    ///Returns a read lock to the shared memory
    ///
    ///The caller must ensure that the index given to this function is valid
    fn rlock<D: SharedMemCast>(
        &self,
        lock_index: usize,
    ) -> Result<ReadLockGuard<D>, SharedMemError>;
    ///Returns a read lock to the shared memory as a slice
    ///
    ///The caller must ensure that the index given to this function is valid
    fn rlock_as_slice<D: SharedMemCast>(
        &self,
        lock_index: usize,
    ) -> Result<ReadLockGuardSlice<D>, SharedMemError>;
}
///Provides wlock/wlock_as_slice functionnalities
pub trait WriteLockable {
    ///Returns a read/write lock to the shared memory
    ///
    ///The caller must ensure that the index given to this function is valid
    fn wlock<D: SharedMemCast>(
        &mut self,
        lock_index: usize,
    ) -> Result<WriteLockGuard<D>, SharedMemError>;
    ///Returns a read/write access to a &mut [T] on the shared memory
    ///
    ///The caller must ensure that the index given to this function is valid
    fn wlock_as_slice<D: SharedMemCast>(
        &mut self,
        lock_index: usize,
    ) -> Result<WriteLockGuardSlice<D>, SharedMemError>;
}
///Provides raw unsafe pointer access
pub trait ReadRaw {
    unsafe fn get_raw<D: SharedMemCast>(&self) -> &D;
    unsafe fn get_raw_slice<D: SharedMemCast>(&self) -> &[D];
}

///Provides raw unsafe pointer access
pub trait WriteRaw {
    unsafe fn get_raw_mut<D: SharedMemCast>(&mut self) -> &mut D;
    unsafe fn get_raw_slice_mut<D: SharedMemCast>(&mut self) -> &mut [D];
}

/* Lock Guards */

///RAII structure used to release the read access of a lock when dropped.
pub struct ReadLockGuard<'a, T: 'a> {
    data: &'a T,
    lock_fn: &'a LockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T: 'a> ReadLockGuard<'a, T> {
    #[doc(hidden)]
    pub fn lock(
        data_ptr: &'a T,
        interface: &'a LockImpl,
        lock_ptr: &'a mut c_void,
    ) -> ReadLockGuard<'a, T> {
        //Acquire the read lock
        interface.rlock(lock_ptr).unwrap();

        ReadLockGuard {
            data: data_ptr,
            lock_fn: interface,
            lock_data: lock_ptr,
        }
    }
}
impl<'a, T: 'a> Drop for ReadLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock_fn.runlock(self.lock_data);
    }
}
impl<'a, T> Deref for ReadLockGuard<'a, T> {
    type Target = &'a T;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

///RAII structure used to release the read access of a lock when dropped.
pub struct ReadLockGuardSlice<'a, T: 'a> {
    data: &'a [T],
    lock_fn: &'a LockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T: 'a> ReadLockGuardSlice<'a, T> {
    #[doc(hidden)]
    pub fn lock(
        data_in: &'a [T],
        lock_fn_in: &'a LockImpl,
        lock_data_in: &'a mut c_void,
    ) -> ReadLockGuardSlice<'a, T> {
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
    fn drop(&mut self) {
        self.lock_fn.runlock(self.lock_data);
    }
}
impl<'a, T> Deref for ReadLockGuardSlice<'a, T> {
    type Target = &'a [T];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

///RAII structure used to release the write access of a lock when dropped.
pub struct WriteLockGuard<'a, T: 'a> {
    data: &'a mut T,
    lock_fn: &'a LockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T: 'a> WriteLockGuard<'a, T> {
    #[doc(hidden)]
    pub fn lock(
        data_ptr: &'a mut T,
        interface: &'a LockImpl,
        lock_ptr: &'a mut c_void,
    ) -> WriteLockGuard<'a, T> {
        //Acquire the write lock
        interface.wlock(lock_ptr).unwrap();

        WriteLockGuard {
            data: data_ptr,
            lock_fn: interface,
            lock_data: lock_ptr,
        }
    }
}
impl<'a, T: 'a> Drop for WriteLockGuard<'a, T> {
    fn drop(&mut self) {
        self.lock_fn.wunlock(self.lock_data);
    }
}
impl<'a, T> Deref for WriteLockGuard<'a, T> {
    type Target = &'a mut T;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl<'a, T> DerefMut for WriteLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut &'a mut T {
        &mut self.data
    }
}

///RAII structure used to release the write access of a lock when dropped.
pub struct WriteLockGuardSlice<'a, T: 'a> {
    data: &'a mut [T],
    lock_fn: &'a LockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T: 'a> WriteLockGuardSlice<'a, T> {
    #[doc(hidden)]
    pub fn lock(
        data_ptr: &'a mut [T],
        interface: &'a LockImpl,
        lock_ptr: &'a mut c_void,
    ) -> WriteLockGuardSlice<'a, T> {
        //Acquire the write lock
        interface.wlock(lock_ptr).unwrap();

        WriteLockGuardSlice {
            data: data_ptr,
            lock_fn: interface,
            lock_data: lock_ptr,
        }
    }
}
impl<'a, T: 'a> Drop for WriteLockGuardSlice<'a, T> {
    fn drop(&mut self) {
        self.lock_fn.wunlock(self.lock_data);
    }
}
impl<'a, T> Deref for WriteLockGuardSlice<'a, T> {
    type Target = &'a mut [T];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}
impl<'a, T> DerefMut for WriteLockGuardSlice<'a, T> {
    fn deref_mut(&mut self) -> &mut &'a mut [T] {
        &mut self.data
    }
}
