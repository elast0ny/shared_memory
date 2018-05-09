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
    pub offset: usize,
    pub length: usize,
    pub data_ptr: *mut c_void,
    pub lock_ptr: *mut c_void,
    pub interface: &'a SharedMemLockImpl,
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
    fn size_of(&self) -> usize {0}
    fn init(&self, _lock_info: &mut GenericLock, _create_new: bool) -> Result<()> {Ok(())}
    fn rlock(&self, _lock_data: *mut c_void) -> Result<()> {Ok(())}
    fn wlock(&self, _lock_data: *mut c_void) -> Result<()> {Ok(())}
    fn runlock(&self, _lock_data: *mut c_void) -> () {}
    fn wunlock(&self, _lock_data: *mut c_void) -> () {}
}
///All locks implement this trait
#[doc(hidden)] pub trait SharedMemLockImpl {
    ///Returns the size of the lock structure that will live in shared memory
    fn size_of(&self) -> usize;
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

///Trait that adds rlock/rlock_as_slice functionnalities
pub trait SharedMemReadLockable {
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
    fn rlock<D: SharedMemCast>(&self, lock_index: usize) -> Result<ReadLockGuard<D>>;
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
    fn rlock_as_slice<D: SharedMemCast>(&self, lock_index: usize) -> Result<ReadLockGuardSlice<D>>;
}
///Trait that adds wlock/wlock_as_slice functionnalities
pub trait SharedMemWriteLockable {
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
    fn wlock<D: SharedMemCast>(&mut self, lock_index: usize) -> Result<WriteLockGuard<D>>;
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
    fn wlock_as_slice<D: SharedMemCast>(&mut self, lock_index: usize) -> Result<WriteLockGuardSlice<D>>;
}


//Implemetation for SharedMem
impl<'a>SharedMemReadLockable for SharedMem<'a> {
    fn rlock<D: SharedMemCast>(&self, lock_index: usize) -> Result<ReadLockGuard<D>> {

        let lock: &GenericLock = &self.conf.lock_data[lock_index];

        //Make sure that we can cast our memory to the type
        let type_size = std::mem::size_of::<D>();
        if type_size > lock.length {
            return Err(From::from(
                format!("Tried to map type of {} bytes to a lock holding only {} bytes", type_size, lock.length)
            ));
        }

        //Return data wrapped in a lock
        Ok(
            //Unsafe required to cast shared memory to our type
            unsafe {
                ReadLockGuard::lock(
                    &(*(lock.data_ptr as *const D)),
                    lock.interface,
                    &mut (*lock.lock_ptr),
                )
            }
        )
    }

    fn rlock_as_slice<D: SharedMemCast>(&self, lock_index: usize) -> Result<ReadLockGuardSlice<D>>{

        let lock: &GenericLock = &self.conf.lock_data[lock_index];

        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > lock.length {
            return Err(From::from(
                format!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size, lock.length)
            ));
        }
        let num_items: usize = lock.length / item_size;

        //Return data wrapped in a lock
        Ok(
            //Unsafe required to cast shared memory to array
            unsafe {
                ReadLockGuardSlice::lock(
                    slice::from_raw_parts(lock.data_ptr as *const D, num_items),
                    lock.interface,
                    &mut (*lock.lock_ptr),
                )
            }
        )
    }
}

impl<'a>SharedMemWriteLockable for SharedMem<'a> {
    fn wlock<D: SharedMemCast>(&mut self, lock_index: usize) -> Result<WriteLockGuard<D>> {

        let lock: &GenericLock = &self.conf.lock_data[lock_index];

        //Make sure that we can cast our memory to the type
        let type_size = std::mem::size_of::<D>();
        if type_size > lock.length {
            return Err(From::from(
                format!("Tried to map type of {} bytes to a lock holding only {} bytes", type_size, lock.length)
            ));
        }

        //Return data wrapped in a lock
        Ok(
            //Unsafe required to cast shared memory to our type
            unsafe {
                WriteLockGuard::lock(
                    &mut (*(lock.data_ptr as *mut D)),
                    lock.interface,
                    &mut (*lock.lock_ptr),
                )
            }
        )
    }

    fn wlock_as_slice<D: SharedMemCast>(&mut self, lock_index: usize) -> Result<WriteLockGuardSlice<D>> {

        let lock: &GenericLock = &self.conf.lock_data[lock_index];

        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > lock.length {
            return Err(From::from(
                format!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size, lock.length)
            ));
        }
        //Calculate how many items our slice will have
        let num_items: usize = lock.length / item_size;

        //Return data wrapped in a lock
        Ok(
            //Unsafe required to cast shared memory to array
            unsafe {
                WriteLockGuardSlice::lock(
                    slice::from_raw_parts_mut((lock.data_ptr as usize + 0) as *mut D, num_items),
                    lock.interface,
                    &mut (*lock.lock_ptr),
                )
            }
        )
    }
}

/* Lock Guards */

///Lock wrappping a non-mutable access to the shared data
pub struct ReadLockGuard<'a, T: 'a> {
    data: &'a T,
    lock_fn: &'a SharedMemLockImpl,
    lock_data: &'a mut c_void,
}
impl<'a, T:'a> ReadLockGuard<'a, T> {
    #[doc(hidden)]
    pub fn lock(data_ptr: &'a T, interface: &'a SharedMemLockImpl, lock_ptr: &'a mut c_void) -> ReadLockGuard<'a, T> {
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
    pub fn lock(data_ptr: &'a mut T, interface: &'a SharedMemLockImpl, lock_ptr: &'a mut c_void) -> WriteLockGuard<'a, T> {
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
    pub fn lock(data_ptr: &'a mut [T], interface: &'a SharedMemLockImpl, lock_ptr: &'a mut c_void) -> WriteLockGuardSlice<'a, T> {
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
