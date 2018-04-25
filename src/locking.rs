//This file provides definitions related to locking in mem_file.

//If you wish to implement your own lock type:
//  1. add a field to the LockType enum bellow
//  2. Go into your OS specific OS.rs and create a new pub struct
//  3. Implement the MemFileLockImpl trait for your new struct
//  4. Make sure that your os_impl::open() and os_impl::create() initialize the lock properly in non-raw mode

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
    //BusyWait,
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

///Trait that all locks need to implement
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

///This trait is implemented by MemFile
pub trait MemFileLockable {
    ///Returns a read lock to the shared memory
    ///
    /// # Examples
    ///
    /// ```
    /// # use mem_file::*;
    /// # use std::path::PathBuf;
    /// # let mut mem_file: MemFile = match MemFile::open(PathBuf::from("shared_mem.link")) {Ok(v) => v, Err(_) => return,};
    /// //let some_val: ReadLockGuard<u8> = mem_file.rlock().unwrap();
    /// let some_val = mem_file.rlock::<u8>().unwrap();
    /// println!("I can read a shared u8 ! {}", *some_val);
    /// ```
    fn rlock<D: MemFileCast>(&self) -> Result<ReadLockGuard<D>>;
    ///Returns a read lock to the shared memory as a slice
    ///
    /// # Examples
    ///
    /// ```
    /// # use mem_file::*;
    /// # use std::path::PathBuf;
    /// # let mut mem_file: MemFile = match MemFile::open(PathBuf::from("shared_mem.link")) {Ok(v) => v, Err(_) => return,};
    /// //let read_buf: ReadLockGuardSlice<u8> = mem_file.rlock_as_slice().unwrap();
    /// let read_buf = mem_file.rlock_as_slice::<u8>().unwrap();
    /// println!("I'm reading into a u8 from a shared &[u8] ! : {}", read_buf[0]);
    /// ```
    fn rlock_as_slice<D: MemFileCast>(&self) -> Result<ReadLockGuardSlice<D>>;
    ///Returns a read/write lock to the shared memory
    /// # Examples
    ///
    /// ```
    /// # use mem_file::*;
    /// # use std::path::PathBuf;
    /// # let mut mem_file: MemFile = match MemFile::open(PathBuf::from("shared_mem.link")) {Ok(v) => v, Err(_) => return,};
    /// //let mut some_val: WriteLockGuard<u32> = mem_file.wlock().unwrap();
    /// let mut some_val = mem_file.wlock::<u32>().unwrap();
    /// *(*some_val) = 1;
    /// ```
    fn wlock<D: MemFileCast>(&mut self) -> Result<WriteLockGuard<D>>;
    ///Returns a read/write access to a &mut [T] on the shared memory
    ///
    /// # Examples
    ///
    /// ```
    /// # use mem_file::*;
    /// # use std::path::PathBuf;
    /// # let mut mem_file: MemFile = match MemFile::open(PathBuf::from("shared_mem.link")) {Ok(v) => v, Err(_) => return,};
    /// //let write_buf: WriteLockGuardSlice<u8> = mem_file.wlock_as_slice().unwrap();
    /// let write_buf = mem_file.wlock_as_slice::<u8>().unwrap();
    /// write_buf[0] = 0x1;
    /// ```
    fn wlock_as_slice<D: MemFileCast>(&mut self) -> Result<WriteLockGuardSlice<D>>;
}

//Implemetation for MemFile
impl<'a>MemFileLockable for MemFile<'a> {
    fn rlock<D: MemFileCast>(&self) -> Result<ReadLockGuard<D>> {

        //Make sure we have a file mapped
        if let Some(ref meta) = self.meta {

            //Make sure that we can cast our memory to the type
            let type_size = std::mem::size_of::<D>();
            if type_size > self.size {
                return Err(From::from(
                    format!("Tried to map MemFile to a too big type {}/{}", type_size, self.size)
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
    fn rlock_as_slice<D: MemFileCast>(&self) -> Result<ReadLockGuardSlice<D>> {

        //Make sure we have a file mapped
        if let Some(ref meta) = self.meta {

            //Make sure that we can cast our memory to the slice
            let item_size = std::mem::size_of::<D>();
            if item_size > self.size {
                return Err(From::from(
                    format!("Tried to map MemFile to a too big type {}/{}", item_size, self.size)
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
    fn wlock<D: MemFileCast>(&mut self) -> Result<WriteLockGuard<D>> {

        //Make sure we have a file mapped
        if let Some(ref mut meta) = self.meta {

            //Make sure that we can cast our memory to the type
            let type_size = std::mem::size_of::<D>();
            if type_size > self.size {
                return Err(From::from(
                    format!("Tried to map MemFile to a too big type {}/{}", type_size, self.size)
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
    fn wlock_as_slice<D: MemFileCast>(&mut self) -> Result<WriteLockGuardSlice<D>> {

        //Make sure we have a file mapped
        if let Some(ref mut meta) = self.meta {

            //Make sure that we can cast our memory to the slice
            let item_size = std::mem::size_of::<D>();
            if item_size > self.size {
                return Err(From::from(
                    format!("Tried to map MemFile to a too big type {}/{}", item_size, self.size)
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
