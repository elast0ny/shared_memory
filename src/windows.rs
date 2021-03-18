use ::winapi::{
    shared::{ntdef::NULL, winerror::ERROR_ALREADY_EXISTS},
    um::{
        errhandlingapi::GetLastError,
        handleapi::CloseHandle,
        memoryapi::{
            CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, VirtualQuery, FILE_MAP_READ,
            FILE_MAP_WRITE,
        },
        winbase::FILE_FLAG_DELETE_ON_CLOSE,
        winnt::{
            DELETE, FILE_ATTRIBUTE_TEMPORARY, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            GENERIC_READ, GENERIC_WRITE, HANDLE, MEMORY_BASIC_INFORMATION, PAGE_READWRITE,
        },
    },
};

#[allow(unused_imports)]
use crate::log::*;
use crate::ShmemError;

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::iter::once;
use std::mem::size_of;
use std::os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle};
use std::path::PathBuf;
use std::ptr::null_mut;

pub struct MapData {
    owner: bool,

    ///The handle to our open mapping
    map_handle: HANDLE,

    /// This file is used for shmem persistence. When an owner wants to drop the mapping,
    /// it opens the file with FILE_FLAG_DELETE_ON_CLOSE, renames the file and closes it.
    /// This makes it so future calls to open the old mapping will fail (as it was renamed) and
    /// deletes the renamed file once all handles have been closed.
    persistent_file: File,

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
        if self.map_ptr as *mut _ != NULL {
            trace!("UnmapViewOfFile(map_ptr:{:p})", self.map_ptr);
            unsafe {
                UnmapViewOfFile(self.map_ptr as *mut _);
            }
        }

        //Close our mapping
        if self.map_handle as *mut _ != NULL {
            trace!("CloseHandle(map_handle:{:p})", self.map_handle);
            unsafe {
                CloseHandle(self.map_handle);
            }
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
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .create(false)
                .attributes(FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE)
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
    match std::fs::create_dir_all(path.as_path()) {
        Ok(_) => Ok(path),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(path),
        Err(e) => Err(ShmemError::UnknownOsError(e.raw_os_error().unwrap() as _)),
    }
}

//Creates a mapping specified by the uid and size
pub fn create_mapping(unique_id: &str, map_size: usize) -> Result<MapData, ShmemError> {
    // Create file to back the shared memory
    let mut file_path = get_tmp_dir()?;
    file_path.push(unique_id.trim_start_matches('/'));
    debug!(
        "Creating persistent_file at {}",
        file_path.to_string_lossy()
    );

    let persistent_file = match OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .create_new(true)
        .attributes(FILE_ATTRIBUTE_TEMPORARY)
        .open(&file_path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(ShmemError::MappingIdExists),
        Err(e) => return Err(ShmemError::MapCreateFailed(e.raw_os_error().unwrap() as _)),
    };

    // Start using MapData ASAP to rely on auto cleanup through Drop
    let mut new_map: MapData = MapData {
        owner: true,
        persistent_file,
        unique_id: String::from(unique_id),
        map_handle: NULL,
        map_size,
        map_ptr: null_mut(),
    };

    //Create Mapping
    debug!("Creating memory mapping");
    let high_size: u32 = ((map_size as u64 & 0xFFFF_FFFF_0000_0000_u64) >> 32) as u32;
    let low_size: u32 = (map_size as u64 & 0xFFFF_FFFF_u64) as u32;
    let unique_id: Vec<u16> = OsStr::new(unique_id).encode_wide().chain(once(0)).collect();
    new_map.map_handle = unsafe {
        CreateFileMappingW(
            new_map.persistent_file.as_raw_handle() as _,
            null_mut(),
            PAGE_READWRITE,
            high_size,
            low_size,
            unique_id.as_ptr(),
        )
    };
    let last_error = unsafe { GetLastError() };
    trace!(
        "CreateFileMappingW({:p}, NULL, {:X}, {}, {}, '{}') == {:p}",
        new_map.persistent_file.as_raw_handle(),
        PAGE_READWRITE,
        high_size,
        low_size,
        new_map.unique_id,
        new_map.map_handle
    );

    if new_map.map_handle == NULL {
        return Err(ShmemError::MapCreateFailed(last_error));
    } else if last_error == ERROR_ALREADY_EXISTS {
        return Err(ShmemError::MappingIdExists);
    }

    //Map mapping into address space
    debug!("Loading mapping into address space");
    new_map.map_ptr =
        unsafe { MapViewOfFile(new_map.map_handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, 0) } as _;

    let last_error = unsafe { GetLastError() };
    trace!(
        "MapViewOfFile({:p}, {:X}, 0, 0, 0) == {:p}",
        new_map.map_handle,
        FILE_MAP_READ | FILE_MAP_WRITE,
        new_map.map_ptr
    );

    if new_map.map_ptr.is_null() {
        return Err(ShmemError::MapCreateFailed(last_error));
    }

    Ok(new_map)
}

//Opens an existing mapping specified by its uid
pub fn open_mapping(unique_id: &str, map_size: usize) -> Result<MapData, ShmemError> {
    let mut file_path = get_tmp_dir()?;
    file_path.push(unique_id.trim_start_matches('/'));
    debug!(
        "Openning persistent_file at {}",
        file_path.to_string_lossy()
    );

    // Open the file backing the shared memory
    let persistent_file = match OpenOptions::new()
        .access_mode(GENERIC_READ | GENERIC_WRITE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .create(false)
        .attributes(FILE_ATTRIBUTE_TEMPORARY)
        .open(&file_path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => return Err(ShmemError::MappingIdExists),
        Err(e) => return Err(ShmemError::MapOpenFailed(e.raw_os_error().unwrap() as _)),
    };

    // Start using MapData ASAP to rely on auto cleanup through Drop
    let mut new_map: MapData = MapData {
        owner: false,
        persistent_file,
        unique_id: String::from(unique_id),
        map_handle: NULL,
        map_size,
        map_ptr: null_mut(),
    };

    // Open the file mapping
    // `CreateFileMappingW` returns the handle to the object even if it
    // exists already and sets ERROR_ALREADY_EXISTS. Ignore the error here.
    debug!("Openning memory mapping");
    let high_size: u32 = ((map_size as u64 & 0xFFFF_FFFF_0000_0000_u64) >> 32) as u32;
    let low_size: u32 = (map_size as u64 & 0xFFFF_FFFF_u64) as u32;
    let unique_id: Vec<u16> = OsStr::new(unique_id).encode_wide().chain(once(0)).collect();
    new_map.map_handle = unsafe {
        CreateFileMappingW(
            new_map.persistent_file.as_raw_handle() as _,
            null_mut(),
            PAGE_READWRITE,
            high_size,
            low_size,
            unique_id.as_ptr(),
        )
    };
    let last_error = unsafe { GetLastError() };
    trace!(
        "CreateFileMappingW({:p}, NULL, {:X}, {}, {}, '{}') == {:p}",
        new_map.persistent_file.as_raw_handle(),
        PAGE_READWRITE,
        high_size,
        low_size,
        new_map.unique_id,
        new_map.map_handle
    );
    if new_map.map_handle == NULL {
        return Err(ShmemError::MapCreateFailed(last_error));
    }

    //Map mapping into address space
    debug!("Loading mapping into address space");
    new_map.map_ptr =
        unsafe { MapViewOfFile(new_map.map_handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, 0) } as _;
    let last_error = unsafe { GetLastError() };
    trace!(
        "MapViewOfFile({:p}, {:X}, 0, 0, 0) == {:p}",
        new_map.map_handle,
        FILE_MAP_READ | FILE_MAP_WRITE,
        new_map.map_ptr
    );
    if new_map.map_ptr.is_null() {
        return Err(ShmemError::MapOpenFailed(last_error));
    }

    //Get the size of our mapping
    new_map.map_size = unsafe {
        let mut mem_ba: MEMORY_BASIC_INFORMATION = MEMORY_BASIC_INFORMATION {
            BaseAddress: null_mut(),
            AllocationBase: null_mut(),
            AllocationProtect: 0,
            RegionSize: 0,
            State: 0,
            Protect: 0,
            Type: 0,
        };
        let ret_val = VirtualQuery(
            new_map.map_ptr as *const _,
            &mut mem_ba as *mut _,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        );

        //Couldnt get mapping size
        if ret_val == 0 {
            let last_error = GetLastError();
            return Err(ShmemError::UnknownOsError(last_error));
        }

        mem_ba.RegionSize
    };

    Ok(new_map)
}
