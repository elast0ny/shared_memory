extern crate winapi;

use super::*;
use self::winapi::shared::ntdef::{NULL};
use self::winapi::shared::minwindef::{FALSE};
use self::winapi::um::winbase::*;
use self::winapi::um::winnt::*;
use self::winapi::um::handleapi::*;
use self::winapi::um::memoryapi::*;
use self::winapi::um::errhandlingapi::*;
use self::winapi::um::synchapi::*;
use std::slice;

use std::mem::size_of;
use std::ffi::CString;
use std::ptr::{null_mut};

use std::fs::File;
use std::io::{Write, Read};

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

///This struct lives insides the shared memory
struct MemCtl {
    ///Lock controlling the access to the mapping
    rw_lock: SRWLOCK,
    ///Actual mapping size, this is set by os_create so that os_open knows the actual requested size
    ///This is required because Windows returns a multiple of PAGE_SIZE from VirtualQuery
    req_size: usize,
}

///This struct describes our memory mapping
pub struct MemMetadata {
    ///Name of mapping
    map_name: String,
    ///Handle of mapping
    map_handle: HANDLE,
    ///Hold data to control the mapping (locks)
    map_ctl: *mut MemCtl,
    ///Holds the actual sizer of the mapping
    map_size: usize,
    ///Pointer to user data
    map_data: *mut c_void,
}

impl MemMetadata {

    /* Get Read Lock Impl */

    //Regular type
    pub fn rlock<T>(&self) -> MemFileRLock<T> {
        unsafe {
            //Acquire read lock
            AcquireSRWLockShared(&mut (*self.map_ctl).rw_lock);
            MemFileRLock {
                data: &(*(self.map_data as *mut T)),
                lock: &mut (*self.map_ctl).rw_lock as *mut _ as *mut c_void,
            }
        }
    }

    ///Gets a reference to the shared memory as a slice of T with size elements
    ///This lock can be held by multiple readers
    ///Caller must validate the parameters
    pub fn rlock_slice<T>(&self, start_offset: usize, num_elements:usize) -> MemFileRLockSlice<T> {
        unsafe {
            //Acquire read lock
            AcquireSRWLockShared(&mut (*self.map_ctl).rw_lock);
            MemFileRLockSlice {
                data: slice::from_raw_parts((self.map_data as usize + start_offset) as *const T, num_elements),
                lock: &mut (*self.map_ctl).rw_lock as *mut _ as *mut c_void,
            }
        }
    }

    /* Get Write Lock Impl */

    //Regular type
    pub fn wlock<T>(&mut self) -> MemFileWLock<T> {
        unsafe {
            //Acquire write lock
            AcquireSRWLockExclusive(&mut (*self.map_ctl).rw_lock);
            MemFileWLock {
                data: &mut (*(self.map_data as *mut T)),
                lock: &mut (*self.map_ctl).rw_lock as *mut _ as *mut c_void,
            }
        }
    }

    ///Gets an exclusive mutable reference to the shared memory
    ///Caller must validate the parameters
    pub fn wlock_slice<T>(&mut self, start_offset: usize, num_elements:usize) -> MemFileWLockSlice<T> {
        unsafe{
            //acquire write lock
            AcquireSRWLockExclusive(&mut (*self.map_ctl).rw_lock);
            MemFileWLockSlice {
                data: slice::from_raw_parts_mut((self.map_data as usize + start_offset) as *mut T, num_elements),
                lock: &mut (*self.map_ctl).rw_lock as *mut _ as *mut c_void,
            }
        }
    }
}

impl Drop for MemMetadata {
    ///Takes care of properly closing the MemFile (munmap(), shmem_unlink(), close())
    fn drop(&mut self) {
        //Unmap memory from our process
        if self.map_ctl as *mut _ == NULL {
            unsafe { UnmapViewOfFile(self.map_ctl as *mut _); }
        }

        //Close our mapping
        if self.map_handle as *mut _ != NULL {
            unsafe { CloseHandle(self.map_handle); }
        }
    }
}

//Opens an existing MemFile, OpenFileMappingA()/MapViewOfFile()/VirtualQuery()
pub fn open(mut new_file: MemFile) -> Result<MemFile> {

    //Get namespace of shared memory
    let map_name: String;
    {
        //Get namespace of shared memory
        let mut disk_file = File::open(&new_file.file_path)?;
        let mut file_contents: Vec<u8> = Vec::with_capacity(new_file.file_path.to_string_lossy().len() + 5);
        disk_file.read_to_end(&mut file_contents)?;
        map_name = String::from_utf8(file_contents)?;
    }

    let mut meta: MemMetadata = MemMetadata {
        map_name: map_name,
        map_handle: null_mut(),
        map_ctl: null_mut(),
        map_size: 0,
        map_data: null_mut(),
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
        meta.map_ctl = MapViewOfFile(
            meta.map_handle,
            FILE_MAP_READ| FILE_MAP_WRITE,
            0,
            0,
            0
        ) as *mut _;
    }

    if meta.map_ctl as *mut _ == NULL {
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
            meta.map_ctl as *const _,
            &mut mem_ba as *mut _,
            size_of::<MEMORY_BASIC_INFORMATION>()
        );

        if ret_val == 0 {
            return Err(From::from(format!("VirtualQuery failed with {}", GetLastError())));
        }

        mem_ba.RegionSize
    };

    new_file.size = unsafe {(*meta.map_ctl).req_size};

    let max_user_data_size = meta.map_size - size_of::<MemCtl>();

    if new_file.size > max_user_data_size {
        new_file.size = max_user_data_size;
    }

    meta.map_data = (meta.map_ctl as usize + size_of::<MemCtl>()) as *mut c_void;
    new_file.meta = Some(meta);

    Ok(new_file)
}

//Creates a new MemFile, CreateFileMappingA()/MapViewOfFile()
pub fn create(mut new_file: MemFile) -> Result<MemFile> {

    let mut disk_file = File::create(&new_file.file_path)?;
    //println!("File created !");
    if !new_file.file_path.is_file() {
        return Err(From::from("Failed to create file"));
    }

    //Get unique name for mem mapping
    let abs_disk_path = new_file.file_path.canonicalize()?;
    let abs_disk_path = abs_disk_path.to_string_lossy();
    let base_drive = match abs_disk_path.find(":") {
        Some(v) => {
            if v == 0 {
                return Err(From::from("Invalid absoluted path"));
            } else {
                v - 1
            }
        },
        None => return Err(From::from("Invalid absoluted path")),
    };
    let abs_disk_path: &str = &abs_disk_path[base_drive..];

    let chars = abs_disk_path.chars();
    let mut unique_name: String = String::with_capacity(abs_disk_path.len());
    //unique_name.push_str("local\\");
    for c in chars {
        match c {
            '\\' | '.' => unique_name.push('_'),
            '?' | ':' => {},
            v => unique_name.push(v),
        };
    }

    let mut meta: MemMetadata = MemMetadata {
        map_name: unique_name,
        map_handle: null_mut(),
        map_ctl: null_mut(),
        map_size: 0,
        map_data: null_mut(),
    };

    //Create mapping and map to our address space
    unsafe {
        let full_size: u64 = (new_file.size + size_of::<MemMetadata>()) as u64;
        let high_size: u32 = (full_size & 0xFFFFFFFF00000000 as u64) as u32;
        let low_size: u32 = (full_size & 0xFFFFFFFF as u64) as u32;
        println!("CreateFileMapping({})", meta.map_name);
        meta.map_handle = CreateFileMappingA(
            INVALID_HANDLE_VALUE,
            null_mut(),
            PAGE_READWRITE,
            high_size,
            low_size,
            CString::new(meta.map_name.clone())?.as_ptr());
    }

    if meta.map_handle == NULL {
        return Err(From::from(format!("CreateFileMappingA failed with {}", unsafe{GetLastError()})));
    }

    unsafe {
        meta.map_ctl = MapViewOfFile(
            meta.map_handle,
            FILE_MAP_READ| FILE_MAP_WRITE,
            0,
            0,
            0
        ) as *mut _;
    }

    if meta.map_ctl as *mut _ == NULL {
        return Err(From::from(format!("MapViewOfFile failed with {}", unsafe{GetLastError()})));
    }

    //initialize lock
    unsafe {
        InitializeSRWLock(&mut (*meta.map_ctl).rw_lock);
        (*meta.map_ctl).req_size = new_file.size;
    }

    //Init pointer to user data
    meta.map_size = new_file.size + size_of::<MemMetadata>();
    meta.map_data = (meta.map_ctl as usize + size_of::<MemCtl>()) as *mut c_void;

    //Write unique shmem name to disk
    match disk_file.write(&meta.map_name.as_bytes()) {
        Ok(write_sz) => if write_sz != meta.map_name.as_bytes().len() {
            return Err(From::from("Failed to write full contents info on disk"));
        },
        Err(_) => return Err(From::from("Failed to write info on disk")),
    };

    new_file.meta = Some(meta);
    Ok(new_file)
}

//Returns a read lock to the shared memory
pub fn rlock<T>(meta: &MemMetadata) -> MemFileRLock<T> {
    return meta.rlock();
}
//Returns an exclusive read/write lock to the shared memory
pub fn wlock<T>(meta: &mut MemMetadata) -> MemFileWLock<T> {
    return meta.wlock();
}

//Returns a read lock to the shared memory as a slice
pub fn rlock_slice<T>(meta: &MemMetadata, start_offset: usize, num_elements:usize) -> MemFileRLockSlice<T> {
    return meta.rlock_slice(start_offset, num_elements);
}
//Returns an exclusive read/write lock to the shared memory as a slice
pub fn wlock_slice<T>(meta: &mut MemMetadata, start_offset: usize, num_elements:usize) -> MemFileWLockSlice<T> {
    return meta.wlock_slice(start_offset, num_elements);
}

//Releases a read lock
pub fn read_unlock(lock_ptr: *mut c_void) {
    unsafe {ReleaseSRWLockShared(lock_ptr as *mut SRWLOCK)};
}
//Releases a write lock
pub fn write_unlock(lock_ptr: *mut c_void) {
    unsafe {ReleaseSRWLockExclusive(lock_ptr as *mut SRWLOCK)};
}
