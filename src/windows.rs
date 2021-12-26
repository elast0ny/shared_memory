use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
use std::path::PathBuf;
use std::ptr::null_mut;

use crate::log::*;
use win_sys::*;
use win_sys::Foundation::INVALID_HANDLE_VALUE;

use crate::ShmemError;

pub struct MapData {
    owner: bool,

    ///The handle to our open mapping
    map_handle: HANDLE,

    /// This file is used for shmem persistence. When an owner wants to drop the mapping,
    /// it opens the file with FILE_FLAG_DELETE_ON_CLOSE, renames the file and closes it.
    /// This makes it so future calls to open the old mapping will fail (as it was renamed) and
    /// deletes the renamed file once all handles have been closed.
    persistent_file: Option<File>,

    //Shared mapping uid
    pub unique_id: String,
    //Total size of the mapping
    pub map_size: usize,
    //Pointer to the first byte of our mapping
    pub map_ptr: *mut u8,
}
///Teardown UnmapViewOfFile and close CreateMapping handle
impl Drop for MapData {
    ///Takes care of properly closing the SharedMem
    fn drop(&mut self) {
        //Unmap memory from our process
        if !self.map_ptr.is_null() {
            trace!("UnmapViewOfFile(map_ptr:{:p})", self.map_ptr);
            let _ = UnmapViewOfFile(self.map_ptr as _);
        }

        //Close our mapping
        if self.map_handle.0 != 0 {
            trace!("CloseHandle(map_handle:0x{:X})", self.map_handle.0);
            let _ = CloseHandle(self.map_handle);
        }

        // Inspired by the boost implementation at
        // https://github.com/boostorg/interprocess/blob/140b50efb3281fa3898f3a4cf939cfbda174718f/include/boost/interprocess/detail/win32_api.hpp
        // Emulate POSIX behavior by
        // 1. Opening the mmapped file with `FILE_FLAG_DELETE_ON_CLOSE`, causing it to be
        // deleted when all its handles have been closed.
        // 2. Renaming the mmapped file to prevent future access/opening.
        // Once this has run, existing file/mapping handles remain usable but the file is
        // deleted once all handles have been closed and no new handles can be opened
        // because the file has been renamed. This matches the behavior of shm_unlink()
        // on unix.
        if self.owner {
            let mut base_path = get_tmp_dir().unwrap();

            // 1. Set file attributes so that it deletes itself once everyone has closed it
            let file_path = base_path.join(self.unique_id.trim_start_matches('/'));
            debug!("Setting mapping to delete after everyone has closed it");
            match OpenOptions::new()
                .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE)
                .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
                .create(false)
                .attributes((FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE).0)
                .open(&file_path)
            {
                Ok(_) => {
                    // 2. Rename file to prevent further use
                    base_path.push(&format!(
                        "{}_deleted",
                        self.unique_id.trim_start_matches('/')
                    ));
                    debug!(
                        "Renaming {} to {}",
                        file_path.to_string_lossy(),
                        base_path.to_string_lossy()
                    );
                    if let Err(_e) = std::fs::rename(&file_path, &base_path) {
                        debug!(
                            "Failed to rename persistent_file {} : {}",
                            file_path.to_string_lossy(),
                            _e
                        );
                    }
                }
                Err(_e) => {
                    debug!(
                        "Failed to set DELETE_ON_CLOSE on persistent_file {} : {}",
                        file_path.to_string_lossy(),
                        _e
                    );
                }
            };
        }
    }
}

impl MapData {
    pub fn set_owner(&mut self, is_owner: bool) -> bool {
        let prev_val = self.owner;
        self.owner = is_owner;
        prev_val
    }
}

/// Returns the path to a temporary directory in which to store files backing the shared memory. If it
/// doesn't exist, the directory is created.
fn get_tmp_dir() -> Result<PathBuf, ShmemError> {
    debug!("Getting & creating shared_memory-rs temp dir");
    let mut path = std::env::temp_dir();
    path.push("shared_memory-rs");

    if path.is_dir() {
        return Ok(path);
    }

    match std::fs::create_dir_all(path.as_path()) {
        Ok(_) => Ok(path),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(path),
        Err(e) => Err(ShmemError::UnknownOsError(e.raw_os_error().unwrap() as _)),
    }
}

fn new_map(unique_id: &str, map_size: usize, create: bool)  -> Result<MapData, ShmemError> {
    let mut f = None;
    let mut map_h = HANDLE(0);

    // Create file to back the shared memory
    let mut file_path = get_tmp_dir()?;
    file_path.push(unique_id.trim_start_matches('/'));
    debug!(
        "{} persistent_file at {}",
        if create {
            "Creating"
        } else {
            "Openning"
        },
        file_path.to_string_lossy()
    );

    let mut opt = OpenOptions::new();
    opt.read(true)
        .write(true)
        .share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE).0)
        .attributes((FILE_ATTRIBUTE_TEMPORARY).0);
    if create {
        opt.create_new(true);
    } else {
        opt.create(false);
    };

    f = match opt.open(&file_path)
    {
        Ok(f) => Some(f),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(ShmemError::MappingIdExists),
        Err(e) => return Err(
            if create {
                ShmemError::MapCreateFailed(e.raw_os_error().unwrap() as _)
            } else {
                debug!("Failed to open persistent posix mapping... Try to open raw shared memory by ID");
                // Allow openning memory mappings not managed by this crate
                // which do not create a file on disk for the mapping
                ShmemError::MapOpenFailed(e.raw_os_error().unwrap() as _)
            }),
    };

    // Start using MapData ASAP to rely on auto cleanup through Drop
    let mut new_map: MapData = MapData {
        owner: create,
        persistent_file: f,
        unique_id: String::from(unique_id),
        map_handle: map_h,
        map_size,
        map_ptr: null_mut(),
    };

    if new_map.map_handle.0 == 0 {
        //Create Mapping
        debug!("{} memory mapping",
            if create {
                "Creating"
            } else {
                "Openning"
            },
        );
        let high_size: u32 = ((map_size as u64 & 0xFFFF_FFFF_0000_0000_u64) >> 32) as u32;
        let low_size: u32 = (map_size as u64 & 0xFFFF_FFFF_u64) as u32;
        trace!(
            "CreateFileMappingW({:?}, NULL, {:X}, {}, {}, '{}') == 0x{:X}",
            new_map.persistent_file,
            PAGE_READWRITE.0,
            high_size,
            low_size,
            new_map.unique_id,
            new_map.map_handle.0
        );

        new_map.map_handle = match CreateFileMappingW(
            if let Some(h) = &new_map.persistent_file {
                HANDLE(h.as_raw_handle() as _)
            } else {
                INVALID_HANDLE_VALUE
            },
            None,
            PAGE_READWRITE,
            high_size,
            low_size,
            unique_id,
        ) {
            Ok(v) => v,
            Err(e) => {
                let err_code = e.win32_error().unwrap();
                return if err_code == ERROR_ALREADY_EXISTS.0 {
                    Err(ShmemError::MappingIdExists)
                } else {
                    Err(if create {
                        ShmemError::MapCreateFailed(err_code)
                    } else {
                        ShmemError::MapOpenFailed(err_code)
                    })
                };
            }
        };

        trace!(
            "CreateFileMappingW({:?}, NULL, {:X}, {}, {}, '{}') == 0x{:X}",
            new_map.persistent_file,
            PAGE_READWRITE.0,
            high_size,
            low_size,
            new_map.unique_id,
            new_map.map_handle.0
        );
    }
    
    //Map mapping into address space
    debug!("Loading mapping into address space");
    trace!(
        "MapViewOfFile(0x{:X}, {:X}, 0, 0, 0)",
        new_map.map_handle.0,
        (FILE_MAP_READ | FILE_MAP_WRITE).0,
    );

    new_map.map_ptr =
        match MapViewOfFile(new_map.map_handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, 0) {
            Ok(v) => v as _,
            Err(e) => return Err(if create {
                ShmemError::MapCreateFailed(e.win32_error().unwrap())
            } else {
                ShmemError::MapOpenFailed(e.win32_error().unwrap())
            }),
        };
    
    trace!(
        "MapViewOfFile(0x{:X}, {:X}, 0, 0, 0) == {:p}",
        new_map.map_handle.0,
        (FILE_MAP_READ | FILE_MAP_WRITE).0,
        new_map.map_ptr
    );

    if !create {
        //Get the real size of the openned mapping
        let mut info = MEMORY_BASIC_INFORMATION::default();
        if let Err(e) = VirtualQuery(new_map.map_ptr as _, &mut info) {
            return Err(ShmemError::UnknownOsError(e.win32_error().unwrap()));
        }
        new_map.map_size = info.RegionSize;
    }

    Ok(new_map)
}

//Creates a mapping specified by the uid and size
pub fn create_mapping(unique_id: &str, map_size: usize) -> Result<MapData, ShmemError> {
    new_map(unique_id, map_size, true)
}

//Opens an existing mapping specified by its uid
pub fn open_mapping(unique_id: &str, map_size: usize) -> Result<MapData, ShmemError> {
    new_map(unique_id, map_size, false)
}
