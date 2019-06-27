//! A user friendly crate that allows you to share memory between __processes__
//!
//! For help on how to get started, take a look at the examples !

#[macro_use]
extern crate cfg_if;
#[macro_use]
extern crate enum_primitive;
#[macro_use]
extern crate log;
#[cfg(feature="fs2")]
extern crate fs2;

use std::ffi::OsStr;
use std::path::{PathBuf, Path};
use std::fs::{File};
use std::fs::remove_file;
use std::slice;
use std::os::raw::c_void;
use std::fmt;

//Lock definitions
mod locks;
pub use locks::*;

//Event definitions
mod events;
pub use events::*;

//Load up the proper OS implementation
cfg_if! {
    if #[cfg(target_os="windows")] {
        mod windows;
        use windows as os_impl;
    } else if #[cfg(any(target_os="freebsd", target_os="linux", target_os="macos"))] {
        mod nix;
        use nix as os_impl;
    } else {
        compile_error!("shared_memory isnt implemented for this platform...");
    }
}

//The alignment of addresses. This affects the alignment of the user data and the
//locks/events in the metadata section.
const ADDR_ALIGN: u8 = 4;

//TODO : Replace this with proper error handling, failure crate maybe
type Result<T> = std::result::Result<T, Box<std::error::Error>>;

///Defines different variants to specify timeouts
pub enum Timeout {
    ///Wait forever for an event to be signaled
    Infinite,
    ///Duration in seconds for a timeout
    Sec(usize),
    ///Duration in milliseconds for a timeout
    Milli(usize),
    ///Duration in microseconds for a timeout
    Micro(usize),
    ///Duration in nanoseconds for a timeout
    Nano(usize),
}

//List of "safe" types that the memory can be cast to
mod cast;
pub use cast::*;

//Implementation of SharedMemConf
mod conf;
pub use conf::*;

//Implementation of SharedMemRaw
mod raw;
pub use raw::*;

///Default shared mapping structure
pub struct SharedMem {
    //Config that describes this mapping
    conf: SharedMemConf,
    //The currently in use link file
    link_file: Option<File>,
    //Os specific data for the mapping
    os_data: os_impl::MapData,
    //User data start address
    user_ptr: *mut c_void,
}
impl SharedMem {

    ///Creates a memory mapping with no link file of specified size controlled by a single lock.
    pub fn create(lock_type: LockType, size: usize) -> Result<SharedMem> {
        SharedMemConf::new()
            .set_size(size)
            .add_lock(lock_type, 0, size).unwrap().create()
    }

    pub fn open(unique_id: &str) -> Result<SharedMem> {
        SharedMemConf::new()
            .set_os_path(unique_id)
            .open()
    }

    pub fn create_linked<I: AsRef<OsStr>>(new_link_path: I, lock_type: LockType, size: usize) -> Result<SharedMem> {
        SharedMemConf::new()
            .set_link_path(new_link_path.as_ref())
            .set_size(size)
            .add_lock(lock_type, 0, size).unwrap().create()
    }
    pub fn open_linked<I: AsRef<OsStr>>(existing_link_path: I) -> Result<SharedMem> {
        SharedMemConf::new()
            .set_link_path(existing_link_path.as_ref())
            .open()
    }

    ///Returns the size of the SharedMem
    #[inline]
    pub fn get_size(&self) -> usize {
        self.conf.get_size()
    }
    #[inline]
    pub fn get_metadata_size(&self) -> usize {
        self.conf.get_metadata_size()
    }
    #[inline]
    pub fn num_locks(&self) -> usize {
        self.conf.num_locks()
    }
    #[inline]
    pub fn num_events(&self) -> usize {
        self.conf.num_events()
    }
    ///Returns the link_path of the SharedMem
    #[inline]
    pub fn get_link_path(&self) -> Option<&Path> {
        self.conf.get_link_path()
    }
    ///Returns the OS specific path of the shared memory object
    ///
    /// Usualy on Linux, this will point to a file under /dev/shm/
    ///
    /// On Windows, this returns a namespace
    #[inline]
    pub fn get_os_path(&self) -> &str {
        &self.os_data.unique_id
    }

    #[inline]
    pub fn get_ptr(&self) -> *mut c_void {
        self.user_ptr
    }
}
impl Drop for SharedMem {

    ///Deletes the SharedMemConf artifacts
    fn drop(&mut self) {

        //Close the openned link file
        drop(&self.link_file);

        //Delete link file if we own it
        if self.conf.is_owner() {
            if let Some(ref file_path) = self.conf.get_link_path() {
                if file_path.is_file() {
                    match remove_file(file_path) {_=>{},};
                }
            }
        }
    }
}
impl fmt::Display for SharedMem {

    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "
        Created : {}
        link : \"{}\"
        os_id : \"{}\"
        MetaSize : {}
        Size : {}
        Num locks : {}
        Num Events : {}
        MetaAddr : {:p}
        UserAddr : {:p}",
            self.conf.is_owner(),
            self.get_link_path().unwrap_or(&PathBuf::from("[NONE]")).to_string_lossy(),
            self.get_os_path(),
            self.get_metadata_size(),
            self.get_size(),
            self.num_locks(),
            self.num_events(),
            self.os_data.map_ptr,
            self.get_ptr(),
        )
    }
}
impl ReadLockable for SharedMem {
    fn rlock<D: SharedMemCast>(&self, lock_index: usize) -> Result<ReadLockGuard<D>> {

        let lock: &GenericLock = self.conf.get_lock(lock_index);

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

        let lock: &GenericLock = self.conf.get_lock(lock_index);

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
impl WriteLockable for SharedMem {
    fn wlock<D: SharedMemCast>(&mut self, lock_index: usize) -> Result<WriteLockGuard<D>> {

        let lock: &GenericLock = self.conf.get_lock(lock_index);

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

        let lock: &GenericLock = self.conf.get_lock(lock_index);

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
impl ReadRaw for SharedMem {
    unsafe fn get_raw<D: SharedMemCast>(&self) -> &D {
        let user_data = self.os_data.map_ptr as usize + self.conf.get_metadata_size();
        return &(*(user_data as *const D))
    }

    unsafe fn get_raw_slice<D: SharedMemCast>(&self) -> &[D] {
        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > self.conf.get_size() {
            panic!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size, self.conf.get_size());
        }
        let num_items: usize = self.conf.get_size() / item_size;
        let user_data = self.os_data.map_ptr as usize + self.conf.get_metadata_size();

        return slice::from_raw_parts(user_data as *const D, num_items);
    }
}
impl WriteRaw for SharedMem {
    unsafe fn get_raw_mut<D: SharedMemCast>(&mut self) -> &mut D {
        let user_data = self.os_data.map_ptr as usize + self.conf.get_metadata_size();
        return &mut (*(user_data as *mut D))
    }
    unsafe fn get_raw_slice_mut<D: SharedMemCast>(&mut self) -> &mut[D] {
        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > self.conf.get_size() {
            panic!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size, self.conf.get_size());
        }
        let num_items: usize = self.conf.get_size() / item_size;
        let user_data = self.os_data.map_ptr as usize + self.conf.get_metadata_size();

        return slice::from_raw_parts_mut(user_data as *mut D, num_items);
    }
}
impl EventSet for SharedMem {
    fn set(&mut self, event_index: usize, state: EventState) -> Result<()> {
        let lock: &GenericEvent = self.conf.get_event(event_index);
        lock.interface.set(lock.ptr, state)
    }
}
impl EventWait for SharedMem {
    fn wait(&self, event_index: usize, timeout: Timeout) -> Result<()> {
        let lock: &GenericEvent = self.conf.get_event(event_index);
        lock.interface.wait(lock.ptr, timeout)
    }
}
