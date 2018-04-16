//#[cfg_attr(debug_assertions, derive(Debug))]

#[macro_use]
extern crate cfg_if;

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
    pub mem_size: usize,
}

impl MemFile {
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
            mem_size: 0,
        };

        MemFile::os_open(mem_file)
    }
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
            mem_size: size,
        };

        MemFile::os_create(mem_file)
    }

    pub fn get_mut_nolock(&self) -> Option<&mut[u8]>{
        if let Some(ref meta) = self.meta {
            meta.get_mut_nolock(self.mem_size)
        } else {
            None
        }
    }
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
