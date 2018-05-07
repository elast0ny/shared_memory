//This file provides definitions related to locking in my_shmem.

//If you wish to implement your own lock type:
//  1. add a field to the LockType enum bellow
//  2. Go into your OS specific OS.rs and create a new pub struct
//  3. Implement the SharedMemLockImpl trait for your new struct
//  4. Make sure that your os_impl::open() and os_impl::create() initialize the lock properly in non-raw mode

use super::*;
use std::ops::{Deref, DerefMut};
use std::os::raw::c_void;

pub struct GenericLock<'a> {
    /* Fields shared in the memory mapping */
    pub uid: u8,
    pub start_ind: usize,
    pub end_ind: usize,

    /* Our internal fields */
    pub size: usize,
    pub ptr: *mut c_void,
    pub interface: &'a SharedMemLockImpl,
}

pub struct GenericEvent {
    /* Fields shared in the memory mapping */
    pub uid: u8,

    /* Our internal fields */
    pub size: usize,
    pub ptr: *mut c_void,
    //TODO : pub interface: &'a SharedMemEventImpl,
}

#[derive(Debug,Copy,Clone)]
///List of all possible locking mechanisms.
///Some OS implementations might not implement all of the possible lock types in this enum.
pub enum LockType {
    ///No locking restrictions on the shared memory
    None = 0,
    ///Only one reader or writer can hold this lock at once
    Mutex = 1,
    ///Multiple readers can access the data. Writer access is exclusive.
    RwLock = 2,
}
#[doc(hidden)]
pub fn lock_uid_to_type(uid: &u8) -> Result<LockType> {
    match *uid {
        0 => Ok(LockType::None),
        1 => Ok(LockType::Mutex),
        2 => Ok(LockType::RwLock),
        _ => Err(From::from("Invalid lock uid")),
    }
}

#[doc(hidden)]
pub struct LockNone {}
impl SharedMemLockImpl for LockNone {
    fn size_of() -> usize {0}
    fn init(&self, _lock_info: &mut GenericLock, create_new: bool) -> Result<()> {Ok(())}
    fn rlock(&self, _lock_data: *mut c_void) -> Result<()> {Ok(())}
    fn wlock(&self, _lock_data: *mut c_void) -> Result<()> {Ok(())}
    fn runlock(&self, _lock_data: *mut c_void) -> () {}
    fn wunlock(&self, _lock_data: *mut c_void) -> () {}
}

///Trait that all locks need to implement
#[doc(hidden)] pub trait SharedMemLockImpl {
    ///Returns the size of this lock structure that should be allocated in the shared mapping
    fn size_of() -> usize where Self: Sized;
    ///Initializes the lock
    fn init(&self, &mut GenericLock, create_new: bool) -> Result<()>;
    ///This method should only return once we have safe read access
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<()>;
    ///This method should only return once we have safe write access
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<()>;
    ///This method is automatically called when a read lock guards is dropped
    fn runlock(&self, lock_ptr: *mut c_void) -> ();
    ///This method is automatically called when a read lock guards is dropped
    fn wunlock(&self, lock_ptr: *mut c_void) -> ();
}

///This trait is implemented by SharedMem
pub trait SharedMemLockable {
    ///Returns a read lock to the shared memory
    ///
    /// # Examples
    ///
    /// ```
    /// # use shared_memory::*;
    /// # use std::path::PathBuf;
    /// # let mut my_shmem: SharedMem = match SharedMem::open(PathBuf::from("shared_mem.link")) {Ok(v) => v, Err(_) => return,};
    /// //let some_val: ReadLockGuard<u8> = my_shmem.rlock().unwrap();
    /// let some_val = my_shmem.rlock::<u8>().unwrap();
    /// println!("I can read a shared u8 ! {}", *some_val);
    /// ```
    fn rlock<D: SharedMemCast>(&self) -> Result<ReadLockGuard<D>>;
    ///Returns a read lock to the shared memory as a slice
    ///
    /// # Examples
    ///
    /// ```
    /// # use shared_memory::*;
    /// # use std::path::PathBuf;
    /// # let mut my_shmem: SharedMem = match SharedMem::open(PathBuf::from("shared_mem.link")) {Ok(v) => v, Err(_) => return,};
    /// //let read_buf: ReadLockGuardSlice<u8> = my_shmem.rlock_as_slice().unwrap();
    /// let read_buf = my_shmem.rlock_as_slice::<u8>().unwrap();
    /// println!("I'm reading into a u8 from a shared &[u8] ! : {}", read_buf[0]);
    /// ```
    fn rlock_as_slice<D: SharedMemCast>(&self) -> Result<ReadLockGuardSlice<D>>;
    ///Returns a read/write lock to the shared memory
    /// # Examples
    ///
    /// ```
    /// # use shared_memory::*;
    /// # use std::path::PathBuf;
    /// # let mut my_shmem: SharedMem = match SharedMem::open(PathBuf::from("shared_mem.link")) {Ok(v) => v, Err(_) => return,};
    /// //let mut some_val: WriteLockGuard<u32> = my_shmem.wlock().unwrap();
    /// let mut some_val = my_shmem.wlock::<u32>().unwrap();
    /// *(*some_val) = 1;
    /// ```
    fn wlock<D: SharedMemCast>(&mut self) -> Result<WriteLockGuard<D>>;
    ///Returns a read/write access to a &mut [T] on the shared memory
    ///
    /// # Examples
    ///
    /// ```
    /// # use shared_memory::*;
    /// # use std::path::PathBuf;
    /// # let mut my_shmem: SharedMem = match SharedMem::open(PathBuf::from("shared_mem.link")) {Ok(v) => v, Err(_) => return,};
    /// //let write_buf: WriteLockGuardSlice<u8> = my_shmem.wlock_as_slice().unwrap();
    /// let write_buf = my_shmem.wlock_as_slice::<u8>().unwrap();
    /// write_buf[0] = 0x1;
    /// ```
    fn wlock_as_slice<D: SharedMemCast>(&mut self) -> Result<WriteLockGuardSlice<D>>;
}

/*
//Implemetation for SharedMem
impl<'a>SharedMemLockable for SharedMem<'a> {
    fn rlock<D: SharedMemCast>(&self) -> Result<ReadLockGuard<D>> {

        //Make sure we have a file mapped
        if let Some(ref meta) = self.meta {

            //Make sure that we can cast our memory to the type
            let type_size = std::mem::size_of::<D>();
            if type_size > self.size {
                return Err(From::from(
                    format!("Tried to map SharedMem to a too big type {}/{}", type_size, self.size)
                ));
            }

            //Return data wrapped in a lock
            Ok(unsafe {
                ReadLockGuard::lock(
                    &(*(meta.data as *const D)),
                    meta.lock_impl,
                    &mut (*meta.lock_data),
                )
            })

        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }
    fn rlock_as_slice<D: SharedMemCast>(&self) -> Result<ReadLockGuardSlice<D>> {

        //Make sure we have a file mapped
        if let Some(ref meta) = self.meta {

            //Make sure that we can cast our memory to the slice
            let item_size = std::mem::size_of::<D>();
            if item_size > self.size {
                return Err(From::from(
                    format!("Tried to map SharedMem to a too big type {}/{}", item_size, self.size)
                ));
            }
            let num_items: usize = self.size / item_size;

            //Return data wrapped in a lock
            Ok(unsafe {
                ReadLockGuardSlice::lock(
                    slice::from_raw_parts((meta.data as usize + 0) as *const D, num_items),
                    meta.lock_impl,
                    &mut (*meta.lock_data),
                )
            })

        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }
    fn wlock<D: SharedMemCast>(&mut self) -> Result<WriteLockGuard<D>> {

        //Make sure we have a file mapped
        if let Some(ref mut meta) = self.meta {

            //Make sure that we can cast our memory to the type
            let type_size = std::mem::size_of::<D>();
            if type_size > self.size {
                return Err(From::from(
                    format!("Tried to map SharedMem to a too big type {}/{}", type_size, self.size)
                ));
            }

            //Return data wrapped in a lock
            Ok(unsafe {
                WriteLockGuard::lock(
                    &mut (*(meta.data as *mut D)),
                    meta.lock_impl,
                    &mut (*meta.lock_data),
                )
            })

        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }
    fn wlock_as_slice<D: SharedMemCast>(&mut self) -> Result<WriteLockGuardSlice<D>> {

        //Make sure we have a file mapped
        if let Some(ref mut meta) = self.meta {

            //Make sure that we can cast our memory to the slice
            let item_size = std::mem::size_of::<D>();
            if item_size > self.size {
                return Err(From::from(
                    format!("Tried to map SharedMem to a too big type {}/{}", item_size, self.size)
                ));
            }
            let num_items: usize = self.size / item_size;

            //Return data wrapped in a lock
            Ok(unsafe {
                WriteLockGuardSlice::lock(
                    slice::from_raw_parts_mut((meta.data as usize + 0) as *mut D, num_items),
                    meta.lock_impl,
                    &mut (*meta.lock_data),
                )
            })
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }
}
*/

/* Lock Guards */

///Lock wrappping a non-mutable access to the shared data
pub struct ReadLockGuard<'a, T: 'a> {
    data: &'a T,
    lock_fn: &'a SharedMemLockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T:'a> ReadLockGuard<'a, T> {
    #[doc(hidden)]
    pub fn lock(data_in: &'a T, lock_fn_in: &'a SharedMemLockImpl, lock_data_in: &'a mut c_void) -> ReadLockGuard<'a, T> {
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
    lock_fn: &'a SharedMemLockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T:'a> ReadLockGuardSlice<'a, T> {
    #[doc(hidden)]
    pub fn lock(data_in: &'a [T], lock_fn_in: &'a SharedMemLockImpl, lock_data_in: &'a mut c_void) -> ReadLockGuardSlice<'a, T> {
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
    lock_fn: &'a SharedMemLockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T:'a> WriteLockGuard<'a, T> {
    #[doc(hidden)]
    pub fn lock(data_in: &'a mut T, lock_fn_in: &'a SharedMemLockImpl, lock_data_in: &'a mut c_void) -> WriteLockGuard<'a, T> {
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
    lock_fn: &'a SharedMemLockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T:'a> WriteLockGuardSlice<'a, T> {
    #[doc(hidden)]
    pub fn lock(data_in: &'a mut [T], lock_fn_in: &'a SharedMemLockImpl, lock_data_in: &'a mut c_void) -> WriteLockGuardSlice<'a, T> {
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
