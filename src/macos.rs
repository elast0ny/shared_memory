compile_error!("MacOs support coming soon");

use super::{std,
    MemFile,
    LockType,
    LockNone,
    MemFileLockImpl,
};

use std::path::PathBuf;
use std::os::raw::c_void;
use std::os::unix::io::RawFd;
use std::ptr::{null_mut};
use std::mem::size_of;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

//This struct will live in the shared memory
struct SharedData {
    //This field is used to transmit the locking mechanism to an openner
    lock_ind: u8,
}

pub struct MemMetadata<'a> {

    /* Optionnal implementation fields */

    ///True if we created the mapping. Need to shm_unlink when we own the link
    owner: bool,
    ///Linux specific shared AsMut
    shared_data: *mut SharedData,
    ///Name of mapping
    map_name: String,
    ///File descriptor from shm_open()
    map_fd: RawFd,
    ///Holds the actual sizer of the mapping
    map_size: usize,

    /* Mandatory fields */
    ///the shared memory for our lock
    pub lock_data: *mut c_void,
    ///Pointer to user data
    pub data: *mut c_void,
    //Our custom lock implementation
    pub lock_impl : &'a MemFileLockImpl,

}

///shared memory teardown for linux
impl<'a> Drop for MemMetadata<'a> {
    ///Takes care of properly closing the MemFile
    fn drop(&mut self) {
    }
}

//Opens an existing MemFile
pub fn open(mut new_file: MemFile) -> Result<MemFile> {
    Ok(new_file)
}

//Creates a new MemFile
pub fn create(mut new_file: MemFile, lock_type: LockType) -> Result<MemFile> {
    Ok(new_file)
}

//Returns the index and size of the lock_type
fn supported_locktype_info(lock_type: &LockType) -> (usize, usize) {
    match lock_type {
        &LockType::None => (0, LockNone::size_of()),
        //&LockType::Mutex => (1, Mutex::size_of()),
        //&LockType::RwLock => (2, RwLock::size_of()),
    }
}

//Returns the proper locktype and size of the structure
fn supported_locktype_from_ind(index: usize) -> (LockType, usize) {
    match index {
        0 => (LockType::None, LockNone::size_of()),
        //1 => (LockType::Mutex, Mutex::size_of()),
        //2 => (LockType::RwLock, RwLock::size_of()),
        _ => unimplemented!("OSX does not support this locktype index..."),
    }
}

/* Lock Implementations */
//Mutex
pub struct Mutex {}
impl MemFileLockImpl for Mutex {

    fn size_of() -> usize {
        0
    }
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        Ok(())
    }
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        Ok(())
    }
    fn runlock(&self, lock_ptr: *mut c_void) -> () {
    }
    fn wunlock(&self, lock_ptr: *mut c_void) -> () {
    }
}

//RwLock
pub struct RwLock {}
impl MemFileLockImpl for RwLock {

    fn size_of() -> usize {
        0
    }
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        Ok(())
    }
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        Ok(())
    }
    fn runlock(&self, lock_ptr: *mut c_void) -> () {
    }
    fn wunlock(&self, lock_ptr: *mut c_void) -> () {
    }
}
