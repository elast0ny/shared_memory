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

    pub fn read_lock(&self) -> Result<MemFileRLock> {

        if let Some(ref meta) = self.meta {
            return Ok(meta.read_lock());
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }

    pub fn write_lock(&mut self) -> Result<MemFileWLock> {

        if let Some(ref mut meta) = self.meta {
            return Ok(meta.write_lock());
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
