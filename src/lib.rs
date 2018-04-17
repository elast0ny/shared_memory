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

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

pub struct MemPermission {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

///Struct used to describe a memory mapped file
pub struct MemFile {
    ///Meta data to help manage this MemFile
    meta: Option<MemMetadata>,
    ///Did we create this MemFile
    pub owner: bool,
    ///Path to the MemFile link on disk
    pub file_path: PathBuf,
    ///Premissions on the MemFile
    pub mem_perm: MemPermission,
    ///Size of the mapping
    pub req_size: usize,
    ///Index into the MemFile
    pub index: usize,
}

impl MemFile {
    ///Opens an existing MemFile
    pub fn open(path: PathBuf, perm: MemPermission) -> Result<MemFile> {

        if !perm.read {
            return Err(From::from("Cannot open MemFile without read permission"));
        }

        if !path.is_file() {
            return Err(From::from("Cannot open MemFile because file doesnt exists"));
        }

        let mem_file: MemFile = MemFile {
            meta: None,
            owner: false,
            file_path: path,
            mem_perm: perm,
            req_size: 0,
            index: 0,
        };

        MemFile::os_open(mem_file)
    }
    ///Creates a new MemFile
    pub fn create(path: PathBuf, perm: MemPermission, size: usize) -> Result<MemFile> {

        if !perm.read {
            return Err(From::from("Cannot create MemFile without read permission"));
        }

        if path.is_file() {
            return Err(From::from("Cannot create MemFile because file already exists"));
        }

        let mem_file: MemFile = MemFile {
            meta: None,
            owner: true,
            file_path: path,
            mem_perm: perm,
            req_size: size,
            index: 0,
        };

        MemFile::os_create(mem_file)
    }

    ///Returns read access to a slice on the shared memory
    pub fn rlock_as_slice<T: MemFileCast>(&self) -> Result<MemFileRLock<T>> {

        //Make sure we have a file mapped
        if let Some(ref meta) = self.meta {

            //Figure out how many elements will be in the slice
            let item_size = std::mem::size_of::<T>();
            if item_size > self.req_size {
                panic!("Tried to map MemFile to a too big type");
            }
            let num_items: usize = self.req_size / item_size;

            return Ok(meta.read_lock_slice::<T>(0, num_items));
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }

    ///Returns exclusive read/write access to a slice on the shared memory
    pub fn wlock_as_slice<T: MemFileCast>(&mut self) -> Result<MemFileWLock<T>> {

        //Make sure we have a file mapped
        if let Some(ref mut meta) = self.meta {

            //Figure out how many elements will be in the slice
            let item_size = std::mem::size_of::<T>();
            if item_size > self.req_size {
                panic!("Tried to map MemFile to a too big type");
            }
            let num_items: usize = self.req_size / item_size;

            return Ok(meta.write_lock_slice::<T>(0, num_items));
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }

    ///Returns a slice pointing to the shared memory
    ///
    ///WARNING : Only use this if you know what you're doing. This is not thread safe.
    ///WARNING : Do not write to this array if you didnt create it with MemPermission.read = true
    pub fn get_mut_nolock(&self) -> Option<&mut[u8]>{
        if let Some(ref meta) = self.meta {
            meta.get_mut_nolock()
        } else {
            None
        }
    }
    /*
    pub fn read(&mut self, buf: &mut [u8]) -> std::result::Result<usize, std::io::Error> {

        let mut bytes_to_read: usize = buf.len();

        if bytes_to_read + self.index >= self.mem_size {
            bytes_to_read = (self.mem_size - 1) - self.index;
        }

        if bytes_to_read == 0 { return Ok(0)}

        if let Some(ref mut meta) = self.meta {
            //Move from src to dst
            {
                let shared_mem = meta.read_lock().data;
                for i in 0..bytes_to_read {
                    buf[i] = shared_mem[self.index + i];
                }
            }
            self.index += bytes_to_read;
            meta.unlock();
        }
        Ok(bytes_to_read)
    }
    pub fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::result::Result<usize, std::io::Error> {
        Ok(0)
    }
    pub fn read_to_string(&mut self, buf: &mut String) -> std::result::Result<usize, std::io::Error> {
        Ok(0)
    }
    pub fn read_exact(&mut self, buf: &mut [u8]) -> std::result::Result<(), std::io::Error> {
        Ok(())
    }

    pub fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let mut bytes_to_write: usize = buf.len();

        if bytes_to_write + self.index >= self.mem_size {
            bytes_to_write = (self.mem_size - 1) - self.index;
        }

        if bytes_to_write == 0 { return Ok(0)}

        if let Some(ref mut meta) = self.meta {
            //Move from src to dst
            {
                let shared_mem = meta.write_lock().unwrap();
                for i in 0..bytes_to_write {
                    shared_mem[self.index + i] = buf[i];
                }
            }
            self.index += bytes_to_write;
            meta.unlock();
        }

        Ok(0)
    }
    */
}

impl Drop for MemFile {
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
