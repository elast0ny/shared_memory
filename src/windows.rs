use ::winapi::{
    shared::{
        ntdef::{FALSE, NULL},
        winerror::ERROR_ALREADY_EXISTS,
    },
    um::{
        errhandlingapi::GetLastError,
        handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
        memoryapi::{MapViewOfFile, UnmapViewOfFile, VirtualQuery, FILE_MAP_READ, FILE_MAP_WRITE},
        winbase::{CreateFileMappingA, OpenFileMappingA},
        winnt::{HANDLE, MEMORY_BASIC_INFORMATION, PAGE_READWRITE},
    },
};

use crate::ShmemError;

use std::ffi::CString;
use std::mem::size_of;
use std::ptr::null_mut;

pub struct MapData {
    ///The handle to our open mapping
    map_handle: HANDLE,

    //Shared mapping uid
    pub unique_id: String,
    //Total size of the mapping
    pub map_size: usize,
    //Pointer to the first byte of our mapping
    pub map_ptr: *mut u8,
}
///Teardown UnmapViewOfFile and close CreateMapping handle
impl Drop for MapData {
    ///Takes care of properly closing the SharedMem (munmap(), shmem_unlink(), close())
    fn drop(&mut self) {
        //Unmap memory from our process
        if self.map_ptr as *mut _ != NULL {
            unsafe {
                UnmapViewOfFile(self.map_ptr as *mut _);
            }
        }

        //Close our mapping
        if self.map_handle as *mut _ != NULL {
            unsafe {
                CloseHandle(self.map_handle);
            }
        }
    }
}

//Creates a mapping specified by the uid and size
pub fn create_mapping(unique_id: &str, map_size: usize) -> Result<MapData, ShmemError> {
    let mut new_map: MapData = MapData {
        unique_id: String::from(unique_id),
        map_handle: NULL,
        map_size: map_size,
        map_ptr: null_mut(),
    };

    //Create Mapping
    new_map.map_handle = unsafe {
        let high_size: u32 = (map_size as u64 & 0xFFFF_FFFF_0000_0000 as u64) as u32;
        let low_size: u32 = (map_size as u64 & 0xFFFF_FFFF as u64) as u32;
        CreateFileMappingA(
            INVALID_HANDLE_VALUE,
            null_mut(),
            PAGE_READWRITE,
            high_size,
            low_size,
            #[allow(clippy::temporary_cstring_as_ptr)]
            CString::new(unique_id).unwrap().as_ptr(),
        )
    };
    let last_error = unsafe { GetLastError() };

    if new_map.map_handle == NULL {
        return Err(ShmemError::MapCreateFailed(last_error));
    } else if last_error == ERROR_ALREADY_EXISTS {
        return Err(ShmemError::MappingIdExists);
    }

    //Map mapping into address space
    new_map.map_ptr =
        unsafe { MapViewOfFile(new_map.map_handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, 0) } as _;
    if new_map.map_ptr.is_null() {
        let last_error = unsafe { GetLastError() };
        unsafe {
            CloseHandle(new_map.map_handle);
        }
        return Err(ShmemError::MapCreateFailed(last_error));
    }

    Ok(new_map)
}

//Opens an existing mapping specified by its uid
pub fn open_mapping(unique_id: &str) -> Result<MapData, ShmemError> {
    let mut new_map: MapData = MapData {
        unique_id: String::from(unique_id),
        map_handle: NULL,
        map_size: 0,
        map_ptr: null_mut(),
    };

    //Open existing mapping
    new_map.map_handle = unsafe {
        OpenFileMappingA(
            FILE_MAP_READ | FILE_MAP_WRITE,
            FALSE as _,
            #[allow(clippy::temporary_cstring_as_ptr)]
            CString::new(unique_id).unwrap().as_ptr(),
        )
    };
    if new_map.map_handle as *mut _ == NULL {
        let last_error = unsafe { GetLastError() };
        return Err(ShmemError::MapOpenFailed(last_error));
    }

    //Map mapping into address space
    new_map.map_ptr =
        unsafe { MapViewOfFile(new_map.map_handle, FILE_MAP_READ | FILE_MAP_WRITE, 0, 0, 0) } as _;
    if new_map.map_ptr.is_null() {
        let last_error = unsafe { GetLastError() };
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
