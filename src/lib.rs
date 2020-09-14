//! A thin wrapper around shared memory system calls
//!
//! For help on how to get started, take a look at the [examples](https://github.com/elast0ny/shared_memory-rs/tree/master/examples) !

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};

use std::fs::remove_file;
use std::path::{Path, PathBuf};

use ::cfg_if::*;

mod error;
pub use error::*;

//Load up the proper OS implementation
cfg_if! {
    if #[cfg(target_os="windows")] {
        mod windows;
        use windows as os_impl;
    } else if #[cfg(any(target_os="freebsd", target_os="linux", target_os="macos"))] {
        mod unix;
        use crate::unix as os_impl;
    } else {
        compile_error!("shared_memory isnt implemented for this platform...");
    }
}

/// Struct used to configure different parameters before creating a shared memory mapping
pub struct ShmemConf {
    owner: bool,
    os_id: Option<String>,
    overwrite_flink: bool,
    flink_path: Option<PathBuf>,
    size: usize,
}
impl Drop for ShmemConf {
    fn drop(&mut self) {
        // Delete the flink if we are the owner of the mapping
        if self.owner {
            if let Some(flink_path) = self.flink_path.as_ref() {
                let _ = remove_file(flink_path);
            }
        }
    }
}
#[allow(clippy::new_without_default)]
impl ShmemConf {
    /// Create a new default shmem config
    pub fn new() -> Self {
        Self {
            owner: false,
            os_id: None,
            overwrite_flink: false,
            flink_path: None,
            size: 0,
        }
    }
    /// Provide a specific os identifier for the mapping
    ///
    /// When not specified, a randomly generated identifier will be used
    pub fn os_id<S: AsRef<str>>(mut self, os_id: S) -> Self {
        self.os_id = Some(String::from(os_id.as_ref()));
        self
    }

    /// Overwrites file links if it already exist when calling `create()`
    pub fn force_create_flink(mut self) -> Self {
        self.overwrite_flink = true;
        self
    }

    /// Create the shared memory mapping with a file link
    ///
    /// This creates a file on disk that contains the unique os_id for the mapping.
    /// This can be useful when application want to rely on filesystems to share mappings
    pub fn flink<S: AsRef<Path>>(mut self, path: S) -> Self {
        self.flink_path = Some(PathBuf::from(path.as_ref()));
        self
    }

    /// Sets the size of the mapping that will be used in `create()`
    pub fn size(mut self, size: usize) -> Self {
        self.size = size;
        self
    }

    /// Create a new mapping using the current configuration
    pub fn create(mut self) -> Result<Shmem, ShmemError> {
        if self.size == 0 {
            return Err(ShmemError::MapSizeZero);
        }

        // Create the mapping
        let mapping = match self.os_id {
            None => {
                // Generate random ID until one works
                loop {
                    let cur_id = format!("/shmem_{:X}", rand::random::<u64>());
                    match os_impl::create_mapping(&cur_id, self.size) {
                        Err(ShmemError::MappingIdExists) => continue,
                        Ok(m) => break m,
                        Err(e) => return Err(e),
                    };
                }
            }
            Some(ref specific_id) => os_impl::create_mapping(specific_id, self.size)?,
        };

        // Create flink
        if let Some(ref flink_path) = self.flink_path {
            let mut open_options: OpenOptions = OpenOptions::new();
            open_options.write(true);
            if self.overwrite_flink {
                open_options.truncate(true);
            } else {
                open_options.create_new(true);
            }

            match open_options.open(flink_path) {
                Ok(mut f) => {
                    // Write the os_id in the flink
                    if let Err(e) = f.write(mapping.unique_id.as_bytes()) {
                        return Err(ShmemError::LinkWriteFailed(e));
                    }
                }
                Err(e) => {
                    return Err(match e.kind() {
                        std::io::ErrorKind::AlreadyExists => ShmemError::LinkExists,
                        _ => ShmemError::LinkCreateFailed(e),
                    });
                }
            };
        }

        self.owner = true;
        self.size = mapping.map_size;

        Ok(Shmem {
            config: self,
            mapping,
        })
    }

    /// Opens an existing mapping using the current configuration
    pub fn open(mut self) -> Result<Shmem, ShmemError> {
        // Must at least have a flink or an os_id
        if self.flink_path.is_none() && self.os_id.is_none() {
            return Err(ShmemError::NoLinkOrOsId);
        }

        // Get the os_id from the flink
        if let Some(ref flink_path) = self.flink_path {
            let mut f = match File::open(flink_path) {
                Ok(f) => f,
                Err(e) => return Err(ShmemError::LinkOpenFailed(e)),
            };
            let mut contents: Vec<u8> = Vec::new();
            if let Err(e) = f.read_to_end(&mut contents) {
                return Err(ShmemError::LinkReadFailed(e));
            }
            let link_os_id = match String::from_utf8(contents) {
                Ok(s) => s,
                Err(_) => return Err(ShmemError::LinkDoesNotExist),
            };
            if let Some(os_id) = self.os_id.as_ref() {
                if *os_id != link_os_id {
                    return Err(ShmemError::FlinkInvalidOsId);
                }
            } else {
                self.os_id = Some(link_os_id);
            }
        }

        let os_id = match self.os_id.as_ref() {
            Some(i) => i,
            None => return Err(ShmemError::NoLinkOrOsId),
        };

        let mapping = os_impl::open_mapping(os_id)?;

        self.size = mapping.map_size;
        self.owner = false;

        Ok(Shmem {
            config: self,
            mapping,
        })
    }
}

/// Structure used to extract information from an existing shared memory mapping
pub struct Shmem {
    config: ShmemConf,
    mapping: os_impl::MapData,
}
#[allow(clippy::len_without_is_empty)]
impl Shmem {
    /// Returns whether we created the mapping or not
    pub fn is_owner(&self) -> bool {
        self.config.owner
    }
    /// Allows for gaining/releasing ownership of the mapping
    ///
    /// Warning : You must ensure at least one process owns the mapping in order to ensure proper cleanup code is ran
    pub fn set_owner(&mut self, is_owner: bool) -> bool {
        
        #[cfg(any(target_os="freebsd", target_os="linux", target_os="macos"))]
        self.mapping.set_owner(is_owner);
        
        let prev_val = self.config.owner;
        self.config.owner = is_owner;
        prev_val
    }
    /// Returns the OS unique identifier for the mapping
    pub fn get_os_id(&self) -> &str {
        self.mapping.unique_id.as_str()
    }
    /// Returns the flink path if present
    pub fn get_flink_path(&self) -> Option<&PathBuf> {
        self.config.flink_path.as_ref()
    }
    /// Returns the total size of the mapping
    pub fn len(&self) -> usize {
        self.mapping.map_size
    }
    /// Returns a raw pointer to the mapping
    pub fn as_ptr(&self) -> *mut u8 {
        self.mapping.map_ptr
    }
    /// Returns mapping as a byte slice
    /// # Safety
    /// This function is unsafe because it is impossible to ensure the range of bytes is immutable
    pub unsafe fn as_slice(&self) -> &[u8] {
        std::slice::from_raw_parts(self.as_ptr(), self.len())
    }
    /// Returns mapping as a mutable byte slice
    /// # Safety
    /// This function is unsafe because it is impossible to ensure the returned mutable refence is unique/exclusive
    pub unsafe fn as_slice_mut(&mut self) -> &mut [u8] {
        std::slice::from_raw_parts_mut(self.as_ptr(), self.len())
    }
}
