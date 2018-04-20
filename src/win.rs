extern crate winapi;

use super::{std,
    MemFile,
    LockType,
    LockNone,
    MemFileLockable};

use self::winapi::shared::ntdef::{NULL};
use self::winapi::shared::minwindef::{FALSE};
use self::winapi::um::winbase::*;
use self::winapi::um::winnt::*;
use self::winapi::um::handleapi::*;
use self::winapi::um::memoryapi::*;
use self::winapi::um::errhandlingapi::*;

use std::path::PathBuf;
use std::mem::size_of;
use std::ffi::CString;
use std::ptr::{null_mut};
use std::os::raw::c_void;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

///This struct describes our memory mapping
pub struct MemMetadata<'a> {

    /* Optionnal implementation fields */

    ///Name of mapping
    map_name: String,
    ///Handle of mapping
    map_handle: HANDLE,
    ///Hold data to control the mapping (locks)
    map_metadata: *mut c_void,
    ///Holds the actual sizer of the mapping
    map_size: usize,

    /* Mandatory fields */

    ///Pointer to user data
    pub data: *mut c_void,
    //Our custom lock implementation
    pub lock : &'a MemFileLockable,
}

/*
AcquireSRWLockShared(&mut (*self.map_ctl).rw_lock);
MemFileRLockSlice {
    data: slice::from_raw_parts((self.map_data as usize + start_offset) as *const T, num_elements),
    lock: &mut (*self.map_ctl).rw_lock as *mut _ as *mut c_void,
}

AcquireSRWLockExclusive(&mut (*self.map_ctl).rw_lock);
MemFileWLock {
    data: &mut (*(self.map_data as *mut T)),
    lock: &mut (*self.map_ctl).rw_lock as *mut _ as *mut c_void,
}

//Releases a read lock
pub fn read_unlock(lock_ptr: *mut c_void) {
    unsafe {ReleaseSRWLockShared(lock_ptr as *mut SRWLOCK)};
}
//Releases a write lock
pub fn write_unlock(lock_ptr: *mut c_void) {
    unsafe {ReleaseSRWLockExclusive(lock_ptr as *mut SRWLOCK)};
}

unsafe {
    InitializeSRWLock(&mut (*(meta.map_metadata as *mut MemCtl)).rw_lock);
    (*(meta.map_metadata as *mut MemCtl)).req_size = new_file.size;
}

*/

///Teardown UnmapViewOfFile and close CreateMapping handle
impl<'a> Drop for MemMetadata<'a> {
    ///Takes care of properly closing the MemFile (munmap(), shmem_unlink(), close())
    fn drop(&mut self) {
        //Unmap memory from our process
        if self.map_metadata as *mut _ == NULL {
            unsafe { UnmapViewOfFile(self.map_metadata as *mut _); }
        }

        //Close our mapping
        if self.map_handle as *mut _ != NULL {
            unsafe { CloseHandle(self.map_handle); }
        }
    }
}

//Opens an existing MemFile, OpenFileMappingA()/MapViewOfFile()/VirtualQuery()
pub fn open(mut new_file: MemFile, lock_type: LockType) -> Result<MemFile> {

    // Get the shmem path
    let mapping_path = match new_file.real_path {
        Some(ref path) => path.clone(),
        None => {
            panic!("Tried to open MemFile with no real_path");
        },
    };

    let map_metadata_sz: usize;
    //Use the proper lock type implementation
    let mut meta: MemMetadata = MemMetadata {
        map_name: mapping_path,
        map_handle: null_mut(),
        map_metadata: null_mut(),
        map_size: 0,
        data: null_mut(),
        lock: match lock_type {
                LockType::None => {
                    map_metadata_sz = 0; /* size_of::<LockShared>() */
                    &LockNone{}
                },
                _ => unimplemented!("Windows only supports LockNone as of now"),
            }
    };

    //Open file specified by namespace
    unsafe {
        meta.map_handle = OpenFileMappingA(
            FILE_MAP_READ| FILE_MAP_WRITE,
            FALSE,
            CString::new(meta.map_name.clone())?.as_ptr()
        );
    }

    if meta.map_handle as *mut _ == NULL {
        return Err(From::from(format!("CreateFileMappingA failed with {}", unsafe{GetLastError()})));
    }

    //Map file to our process memory
    unsafe {
        meta.map_metadata = MapViewOfFile(
            meta.map_handle,
            FILE_MAP_READ| FILE_MAP_WRITE,
            0,
            0,
            0
        ) as *mut _;
    }

    if meta.map_metadata as *mut _ == NULL {
        return Err(From::from(format!("MapViewOfFile failed with {}", unsafe{GetLastError()})));
    }

    //Get the size of our mapping
    meta.map_size = unsafe {
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
            meta.map_metadata as *const _,
            &mut mem_ba as *mut _,
            size_of::<MEMORY_BASIC_INFORMATION>()
        );

        if ret_val == 0 {
            return Err(From::from(format!("VirtualQuery failed with {}", GetLastError())));
        }

        mem_ba.RegionSize
    };

    //new_file.size = unsafe {(*(meta.map_metadata as *mut SharedMeta)).req_size};

    new_file.size = meta.map_size - map_metadata_sz;

    meta.data = (meta.map_metadata as usize + map_metadata_sz) as *mut c_void;
    new_file.meta = Some(meta);

    Ok(new_file)
}

//Creates a new MemFile, CreateFileMappingA()/MapViewOfFile()
pub fn create(mut new_file: MemFile, lock_type: LockType) -> Result<MemFile> {

    // real_path is either :
    // 1. Specified directly
    // 2. Needs to be generated (link_file needs to exist)
    let real_path: String = match new_file.real_path {
        Some(ref path) => path.clone(),
        None => {
            //We dont have a real path and a link file wasn created
            if let Some(ref file_path) = new_file.link_path {
                if !file_path.is_file() {
                    return Err(From::from("os_impl::create() on a link but not link file exists..."));
                }

                //Get unique name for shmem object
                let abs_disk_path: PathBuf = file_path.canonicalize()?;
                let chars = abs_disk_path.to_string_lossy();
                let mut unique_name: String = String::with_capacity(chars.len());
                let mut chars = chars.chars();
                chars.next();
                //unique_name.push_str("local\\");
                for c in chars {
                    match c {
                        '\\' | '.' => unique_name.push('_'),
                        '?' | ':' => {},
                        v => unique_name.push(v),
                    };
                }
                unique_name
            } else {
                //lib.rs shouldnt call us without either real_path or link_path set
                panic!("Trying to create MemFile without any name");
            }
        }
    };

    //Create mapping and map to our address space
    let map_handle;
    unsafe {
        let full_size: u64 = (new_file.size + size_of::<MemMetadata>()) as u64;
        let high_size: u32 = (full_size & 0xFFFFFFFF00000000 as u64) as u32;
        let low_size: u32 = (full_size & 0xFFFFFFFF as u64) as u32;
        println!("CreateFileMapping({})", real_path);
        map_handle = CreateFileMappingA(
            INVALID_HANDLE_VALUE,
            null_mut(),
            PAGE_READWRITE,
            high_size,
            low_size,
            CString::new(real_path.clone())?.as_ptr());
    }

    if map_handle == NULL {
        return Err(From::from(format!("CreateFileMappingA failed with {}", unsafe{GetLastError()})));
    }

    new_file.real_path = Some(real_path.clone());

    let map_metadata_sz: usize;
    //Use the proper lock type implementation
    let mut meta: MemMetadata = MemMetadata {
        map_name: real_path,
        map_handle: map_handle,
        map_metadata: null_mut(),
        map_size: 0,
        data: null_mut(),
        lock: match lock_type {
                LockType::None => {
                    map_metadata_sz = 0; /* size_of::<LockShared>() */
                    &LockNone{}
                },
                _ => unimplemented!("Windows only supports LockNone as of now"),
            }
    };

    unsafe {
        meta.map_metadata = MapViewOfFile(
            meta.map_handle,
            FILE_MAP_READ| FILE_MAP_WRITE,
            0,
            0,
            0
        ) as *mut _;
    }

    if meta.map_metadata as *mut _ == NULL {
        return Err(From::from(format!("MapViewOfFile failed with {}", unsafe{GetLastError()})));
    }

    //initialize lock
    //TODO : Figure out what kind of lock to use for windows


    //Init pointer to user data
    meta.map_size = new_file.size + map_metadata_sz;
    meta.data = (meta.map_metadata as usize + map_metadata_sz) as *mut c_void;

    new_file.meta = Some(meta);
    Ok(new_file)
}
