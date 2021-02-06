use ::winapi::{
    shared::{
        minwindef::{BOOL, DWORD, LPVOID, MAX_PATH},
        ntdef::{FALSE, NULL},
        winerror::{
            ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_FILE_NOT_FOUND,
        },
    },
    um::{
        errhandlingapi::GetLastError,
        fileapi::{
            CreateFileW, GetTempPathW, SetFileInformationByHandle, CREATE_NEW, FILE_RENAME_INFO,
            OPEN_EXISTING,
        },
        handleapi::{CloseHandle, INVALID_HANDLE_VALUE},
        memoryapi::{
            CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, VirtualQuery,
            FILE_MAP_READ, FILE_MAP_WRITE,
        },
        minwinbase::FileRenameInfo,
        winbase::FILE_FLAG_DELETE_ON_CLOSE,
        winnt::{
            DELETE, FILE_ATTRIBUTE_TEMPORARY, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            GENERIC_READ, GENERIC_WRITE, HANDLE, MEMORY_BASIC_INFORMATION, PAGE_READWRITE, WCHAR,
        },
    },
};

use crate::ShmemError;

use std::ffi::{OsStr, OsString};
use std::io::ErrorKind;
use std::iter::once;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;
use std::ptr::null_mut;

pub struct MapData {
    owner: bool,
    file_handle: HANDLE,
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

        // Close the handle to the backing file
        if self.file_handle as *mut _ != NULL {
            unsafe {
                CloseHandle(self.file_handle);
            }
        }

        // Inspired by the boost implementation at
        // https://github.com/boostorg/interprocess/blob/140b50efb3281fa3898f3a4cf939cfbda174718f/include/boost/interprocess/detail/win32_api.hpp
        // Emulate POSIX behavior by
        // 1. Opening the backing file with `FILE_FLAG_DELETE_ON_CLOSE`, causing it to be
        // deleted when all its handles have been closed.
        // 2. Renaming the backing file to prevent future access/opening.
        // Once this has run, existing file/mapping handles remain usable but the file is
        // deleted once all handles have been closed and no new handles can be opened
        // because the file has been renamed. This matches the behavior of shm_unlink()
        // on unix.
        if self.owner {
            let mut base_path = get_tmp_dir().unwrap();
            base_path.push(self.unique_id.trim_start_matches("/"));
            // Encode the file path as a null-terminated UTF-16 sequence
            let file_path: Vec<WCHAR> = base_path
                .as_os_str()
                .encode_wide()
                .chain(OsStr::new("\0").encode_wide())
                .collect();
            let handle = unsafe {
                CreateFileW(
                    file_path.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE | DELETE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    null_mut(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE,
                    NULL,
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                // If `GetLastError() == ERROR_FILE_NOT_FOUND` here the shared memory has already
                // been deleted somewhere else (a different owner has been dropped).
                // Do we need to close the handle here?
                unsafe {
                    CloseHandle(handle);
                }
                let last_error = unsafe { GetLastError() };
                if last_error != ERROR_FILE_NOT_FOUND {
                    Err(ShmemError::UnknownOsError(last_error)).unwrap()
                }
            } else {
                // The backing file still exists and has not been renamed, but may be renamed by the time
                // `SetFileInformationByHandle` is called. Handle that case with an error at the call site.
                base_path.pop();
                base_path.push(format!("{}_delete", self.unique_id.trim_start_matches("/")));
                let new_filename: Vec<WCHAR> = base_path
                    .as_os_str()
                    .encode_wide()
                    .chain(OsStr::new("\0").encode_wide())
                    .collect();
                // Allocate bytes to hold `rename_info` plus `new_filename` in the `FileName` field,
                // see https://github.com/retep998/winapi-rs/issues/231
                // `- 1` takes care of double-counting one element in `new_filename`
                let buf_size =
                    size_of::<FILE_RENAME_INFO>() + size_of::<WCHAR>() * (new_filename.len() - 1);
                let mut buf = vec![0u8; buf_size];
                let rename_info = unsafe { &mut *(buf.as_mut_ptr() as *mut FILE_RENAME_INFO) };
                // See https://stackoverflow.com/questions/36450222/moving-a-file-using-setfileinformationbyhandle
                // for the required inputs.
                rename_info.ReplaceIfExists = true as BOOL;
                rename_info.RootDirectory = NULL;
                // Length without terminating null. Is ignored according to the link above.
                rename_info.FileNameLength =
                    (size_of::<WCHAR>() * (new_filename.len() - 0)) as DWORD;
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        new_filename.as_ptr(),
                        rename_info.FileName.as_mut_ptr(),
                        new_filename.len(),
                    );
                }
                unsafe {
                    debug_assert_eq!(
                        new_filename.as_slice(),
                        std::slice::from_raw_parts(
                            rename_info.FileName.as_ptr(),
                            new_filename.len()
                        )
                    );
                };
                // Rename the backing file
                if unsafe {
                    SetFileInformationByHandle(
                        handle,
                        FileRenameInfo,
                        rename_info as *mut _ as LPVOID,
                        buf_size as DWORD,
                    )
                } == 0
                {
                    let last_error = unsafe { GetLastError() };
                    // The file may have been renamed somewhere else in the meantime,
                    // causing ERROR_ACCESS_DENIED.
                    if last_error != ERROR_ACCESS_DENIED {
                        Err(ShmemError::UnknownOsError(last_error)).unwrap()
                    }
                }
                // Close the file handle
                if unsafe { CloseHandle(handle) } == 0 {
                    let last_error = unsafe { GetLastError() };
                    Err(ShmemError::UnknownOsError(last_error)).unwrap()
                }
            }
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
    let mut buffer: [WCHAR; MAX_PATH] = [0; MAX_PATH];
    let len = unsafe { GetTempPathW(MAX_PATH as DWORD, buffer.as_mut_ptr()) };
    let mut path: PathBuf = if len == 0 {
        let last_error = unsafe { GetLastError() };
        return Err(ShmemError::WindowsTempDirError(Some(last_error)));
    } else if len > MAX_PATH as DWORD {
        let mut buffer = Vec::with_capacity(len as usize);
        let new_len = unsafe { GetTempPathW(len, buffer.as_mut_ptr()) };
        if new_len == 0 {
            let last_error = unsafe { GetLastError() };
            return Err(ShmemError::WindowsTempDirError(Some(last_error)));
        } else if new_len > len {
            unreachable!()
        } else {
            OsString::from_wide(&buffer[..new_len as usize]).into()
        }
    } else {
        OsString::from_wide(&buffer[..len as usize]).into()
    };
    path.push("shared_memory-rs");
    match std::fs::create_dir(path.as_path()) {
        Ok(_) => Ok(path),
        Err(err) => match err.kind() {
            ErrorKind::AlreadyExists => Ok(path),
            _ => Err(ShmemError::WindowsTempDirError(None)),
        },
    }
}

//Creates a mapping specified by the uid and size
pub fn create_mapping(unique_id: &str, map_size: usize) -> Result<MapData, ShmemError> {
    //In addition to being the return value, the Drop impl of this helps clean up on failure
    let mut new_map: MapData = MapData {
        // Set this to true just before returning to avoid running non-cleanup code on drop.
        owner: false,
        file_handle: NULL,
        unique_id: String::from(unique_id),
        map_handle: NULL,
        map_size,
        map_ptr: null_mut(),
    };

    // Create file to back the shared memory
    let mut base_path = get_tmp_dir()?;
    base_path.push(unique_id.trim_start_matches("/"));
    // Encode the file path as a null-terminated UTF-16 sequence
    let file_path: Vec<WCHAR> = base_path
        .as_os_str()
        .encode_wide()
        .chain(OsStr::new("\0").encode_wide())
        .collect();
    new_map.file_handle = unsafe {
        CreateFileW(
            file_path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null_mut(),
            CREATE_NEW,
            FILE_ATTRIBUTE_TEMPORARY,
            NULL,
        )
    };
    if new_map.file_handle == INVALID_HANDLE_VALUE {
        let last_error = unsafe { GetLastError() };
        if last_error == ERROR_FILE_EXISTS {
            return Err(ShmemError::MappingIdExists);
        } else {
            return Err(ShmemError::MapCreateFailed(last_error));
        }
    }

    //Create Mapping
    new_map.map_handle = unsafe {
        let high_size: u32 = ((map_size as u64 & 0xFFFF_FFFF_0000_0000_u64) >> 32) as u32;
        let low_size: u32 = (map_size as u64 & 0xFFFF_FFFF_u64) as u32;
        let unique_id: Vec<u16> = OsStr::new(unique_id).encode_wide().chain(once(0)).collect();
        CreateFileMappingW(
            new_map.file_handle,
            null_mut(),
            PAGE_READWRITE,
            high_size,
            low_size,
            unique_id.as_ptr(),
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
        return Err(ShmemError::MapCreateFailed(last_error));
    }

    new_map.owner = true;
    Ok(new_map)
}

//Opens an existing mapping specified by its uid
pub fn open_mapping(unique_id: &str, map_size: usize) -> Result<MapData, ShmemError> {
    //In addition to being the return value, the Drop impl of this helps clean up on failure
    let mut new_map: MapData = MapData {
        owner: false,
        file_handle: NULL,
        unique_id: String::from(unique_id),
        map_handle: NULL,
        map_size: 0,
        map_ptr: null_mut(),
    };

    //Open existing mapping
    // See https://docs.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-createfilemappingw
    // for why we don't need to flush the file views to disk to share the same memory between processes.
    // "...file views derived from any file mapping object that is backed by the same file are coherent or
    // identical at a specific time."
    new_map.map_handle = unsafe {
        let unique_id: Vec<u16> = OsStr::new(unique_id).encode_wide().chain(once(0)).collect();
        // This call can fail with `ERROR_FILE_NOT_FOUND` if the mapping has been deleted because there
        // are no active handles. In that case we fall back to getting a new handle from the backing
        // file, see below.
        OpenFileMappingW(
            FILE_MAP_READ | FILE_MAP_WRITE,
            FALSE as _,
            unique_id.as_ptr(),
        )
    };
    if new_map.map_handle as *mut _ == NULL {
        let last_error = unsafe { GetLastError() };
        if last_error == ERROR_FILE_NOT_FOUND {
            // The mapping doesn't exist, create one from the backing file.
            let mut base_path = get_tmp_dir().unwrap();
            base_path.push(unique_id.trim_start_matches("/"));
            // Encode the file path as a null-terminated UTF-16 sequence
            let file_path: Vec<WCHAR> = base_path
                .as_os_str()
                .encode_wide()
                .chain(OsStr::new("\0").encode_wide())
                .collect();
            new_map.file_handle = unsafe {
                CreateFileW(
                    file_path.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    null_mut(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_TEMPORARY,
                    NULL,
                )
            };
            if new_map.file_handle == INVALID_HANDLE_VALUE {
                // The backing file doesn't exist.
                let last_error = unsafe { GetLastError() };
                return Err(ShmemError::MapOpenFailed(last_error));
            } else {
                new_map.map_handle = unsafe {
                    let high_size: u32 =
                        ((map_size as u64 & 0xFFFF_FFFF_0000_0000_u64) >> 32) as u32;
                    let low_size: u32 = (map_size as u64 & 0xFFFF_FFFF_u64) as u32;
                    let unique_id: Vec<u16> =
                        OsStr::new(unique_id).encode_wide().chain(once(0)).collect();
                    CreateFileMappingW(
                        new_map.file_handle,
                        null_mut(),
                        PAGE_READWRITE,
                        high_size,
                        low_size,
                        unique_id.as_ptr(),
                    )
                };
                // Unlike in `create_mapping` no need to check for `ERROR_ALREADY_EXISTS`, `CreateFileMappingW`
                // returns the handle to the existing object if one exists. This might be the case if processes
                // race for mapping creation between here and the `OpenFileMappingW` call above.
                if new_map.map_handle == NULL {
                    let last_error = unsafe { GetLastError() };
                    return Err(ShmemError::MapCreateFailed(last_error));
                }
            }
        } else {
            // `OpenFileMappingW` has failed with something else than `ERROR_FILE_NOT_FOUND`.
            return Err(ShmemError::MapOpenFailed(last_error));
        }
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
