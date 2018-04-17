//#[cfg_attr(debug_assertions, derive(Debug))]

#[macro_use]
extern crate cfg_if;

//Load up the proper implementations
cfg_if! {
    if #[cfg(windows)] {
        pub mod win;
        pub use win::*;
    } else if #[cfg(unix)] {
        pub mod nix;
        pub use nix::*;
    } else {
        compile_error!("This library isnt implemented for this platform...");
    }
}

use std::path::PathBuf;
use std::fs::remove_file;
use std::os::raw::c_void;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

///Struct used to describe a memory mapped file
pub struct MemFile {
    ///Meta data to help manage this MemFile
    meta: Option<MemMetadata>,
    ///Did we create this MemFile
    owner: bool,
    ///Path to the MemFile link on disk
    file_path: PathBuf,
    ///Size of the mapping
    size: usize,
}

impl MemFile {
    ///Opens an existing MemFile
    pub fn open(path: PathBuf) -> Result<MemFile> {

        if !path.is_file() {
            return Err(From::from("Cannot open MemFile because file doesnt exists"));
        }

        let mem_file: MemFile = MemFile {
            meta: None,
            owner: false,
            file_path: path,
            size: 0, //os_open needs to fill this field up
        };

        MemFile::os_open(mem_file)
    }
    ///Creates a new MemFile
    pub fn create(path: PathBuf, size: usize) -> Result<MemFile> {

        if path.is_file() {
            return Err(From::from("Cannot create MemFile because file already exists"));
        }

        let mem_file: MemFile = MemFile {
            meta: None,
            owner: true,
            file_path: path,
            size: size,
        };

        MemFile::os_create(mem_file)
    }

    ///Returns read access to a slice on the shared memory
    pub fn rlock_as_slice<T: MemFileCast>(&self) -> Result<MemFileRLockSlice<T>> {

        //Make sure we have a file mapped
        if let Some(ref meta) = self.meta {

            //Figure out how many elements will be in the slice
            let item_size = std::mem::size_of::<T>();
            if item_size > self.size {
                panic!("Tried to map MemFile to a too big type {}/{}", item_size, self.size);
            }
            let num_items: usize = self.size / item_size;

            return Ok(meta.read_lock_slice::<T>(0, num_items));
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }

    ///Returns exclusive read/write access to a slice on the shared memory
    pub fn wlock_as_slice<T: MemFileCast>(&mut self) -> Result<MemFileWLockSlice<T>> {

        //Make sure we have a file mapped
        if let Some(ref mut meta) = self.meta {

            //Figure out how many elements will be in the slice
            let item_size = std::mem::size_of::<T>();
            if item_size > self.size {
                panic!("Tried to map MemFile to a too big type");
            }
            let num_items: usize = self.size / item_size;

            return Ok(meta.write_lock_slice::<T>(0, num_items));
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }
}

impl Drop for MemFile {
    ///Deletes the MemFile artifacts
    fn drop(&mut self) {
        //Delete file on disk if we created it
        if self.owner && self.file_path.is_file() {
            match remove_file(&self.file_path) {_=>{},};
        }
        //Drop our internal view of the MemFile
        if let Some(meta) = self.meta.take() {
            drop(meta);
        }
    }
}

//Different types we support to cast to our shared memory
//Do not change this unless you understand why some types cant be casted onto the shared memory
//A good example would be Vec. As of [today], you cant initialise the Vec struct onto an arbitrary addr.
//That means that if you cast a Vec to the MemFile, it wont be valid data representing the Vec.
//Given we could setup the memory to represent a real vector with its data pointing to our shared memory,
//a bigger problem would be that the Vec could try to resize its data (which is not allocated through normal means).

// We support types that :
// 1. Dont have metadata or can initialise their metadata at an arbitrary address
// 2. Dont internaly use pointers (and no memory allocation/free)
#[doc(hidden)] pub trait MemFileCast {}
#[doc(hidden)] impl MemFileCast for bool {}
#[doc(hidden)] impl MemFileCast for char {}
#[doc(hidden)] impl MemFileCast for str {}
#[doc(hidden)] impl MemFileCast for i8 {}
#[doc(hidden)] impl MemFileCast for i16 {}
#[doc(hidden)] impl MemFileCast for i64 {}
#[doc(hidden)] impl MemFileCast for i32 {}
#[doc(hidden)] impl MemFileCast for u8 {}
#[doc(hidden)] impl MemFileCast for u16 {}
#[doc(hidden)] impl MemFileCast for u32 {}
#[doc(hidden)] impl MemFileCast for u64 {}
#[doc(hidden)] impl MemFileCast for isize {}
#[doc(hidden)] impl MemFileCast for usize {}
#[doc(hidden)] impl MemFileCast for f32 {}
#[doc(hidden)] impl MemFileCast for f64 {}

use std::ops::{Deref, DerefMut};

//Read lock holding a slice
pub struct MemFileRLockSlice<'a, T: 'a> {
    data: &'a [T],
    lock: *mut c_void,
}
impl<'a, T> Drop for MemFileRLockSlice<'a, T> {
    fn drop(&mut self) {
        self.os_unlock();
    }
}
impl<'a, T> Deref for MemFileRLockSlice<'a, T> {
    type Target = &'a [T];
    fn deref(&self) -> &Self::Target { &self.data }
}

//Write lock holding a slice
pub struct MemFileWLockSlice<'a, T: 'a> {
    data: &'a mut [T],
    lock: *mut c_void,
}
impl<'a, T> Drop for MemFileWLockSlice<'a, T> {
    fn drop(&mut self) {
        self.os_unlock();
    }
}
impl<'a, T> Deref for MemFileWLockSlice<'a, T> {
    type Target = &'a mut [T];
    fn deref(&self) -> &Self::Target { &self.data }
}
impl<'a, T> DerefMut for MemFileWLockSlice<'a, T> {
    fn deref_mut(&mut self) -> &mut &'a mut [T] {
        &mut self.data
    }
}
