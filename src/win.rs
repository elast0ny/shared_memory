extern crate winapi;

use self::winapi::shared::ntdef::{NULL};
use self::winapi::shared::minwindef::{FALSE};
use self::winapi::um::winbase::*;
use self::winapi::um::winnt::*;
use self::winapi::um::handleapi::*;
use self::winapi::um::memoryapi::*;
use self::winapi::um::errhandlingapi::*;

use super::{std,
    MemFile,
    LockType,
    LockNone,
    MemFileLockImpl,
};

use std::path::PathBuf;
use std::mem::size_of;
use std::ffi::CString;
use std::ptr::{null_mut};
use std::os::raw::c_void;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

//Theres probably a macro that would do this for me ?
fn ind_to_locktype(ind: &usize) -> LockType {
    match *ind {
        0 => LockType::None,
        1 => LockType::Mutex,
        2 => LockType::Rwlock,
        _ => LockType::None,
    }
}
fn locktype_to_ind(lock_type: &LockType) -> usize {
    match *lock_type {
        LockType::None => 0,
        LockType::Mutex => 1,
        LockType::Rwlock => 2,
    }
}


struct SharedData {
    //This field is used to transmit the locking mechanism to an openner
    lock_ind: u8,
    mapping_size: usize,
}

///This struct describes our memory mapping
pub struct MemMetadata<'a> {

    /* Optionnal implementation fields */

    ///Name of mapping
    /* Windows doesnt care about this */
    //map_name: String,
    ///Handle of mapping
    map_handle: HANDLE,
    ///Hold data to control the mapping (locks)
    shared_data: *mut SharedData,
    ///Holds the actual sizer of the mapping
    /* Windows doesnt care about this */
    //map_size: usize,

    /* Mandatory fields */
    ///the shared memory for our lock
    pub lock_data: *mut c_void,
    ///Pointer to user data
    pub data: *mut c_void,
    //Our custom lock implementation
    pub lock_impl : &'a MemFileLockImpl,
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
        if self.shared_data as *mut _ == NULL {
            unsafe { UnmapViewOfFile(self.shared_data as *mut _); }
        }

        //Close our mapping
        if self.map_handle as *mut _ != NULL {
            unsafe { CloseHandle(self.map_handle); }
        }
    }
}

//Opens an existing MemFile, OpenFileMappingA()/MapViewOfFile()/VirtualQuery()
pub fn open(mut new_file: MemFile) -> Result<MemFile> {

    // Get the shmem path
    let mapping_path = match new_file.real_path {
        Some(ref path) => path.clone(),
        None => {
            panic!("Tried to open MemFile with no real_path");
        },
    };

    //Open file specified by namespace
    let map_handle = unsafe {
        OpenFileMappingA(
            FILE_MAP_READ| FILE_MAP_WRITE,
            FALSE,
            CString::new(mapping_path.clone())?.as_ptr()
        )
    };

    if map_handle as *mut _ == NULL {
        return Err(From::from(format!("CreateFileMappingA failed with {}", unsafe{GetLastError()})));
    }

    new_file.real_path = Some(mapping_path.clone());

    //Map file to our process memory
    let map_addr = unsafe {
        MapViewOfFile(
            map_handle,
            FILE_MAP_READ| FILE_MAP_WRITE,
            0,
            0,
            0
        )
    };

    if map_addr == NULL {
        unsafe { CloseHandle(map_handle); }
        return Err(From::from(format!("MapViewOfFile failed with {}", unsafe{GetLastError()})));
    }

    //Get the size of our mapping
    let full_size = unsafe {
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
            map_addr as *const _,
            &mut mem_ba as *mut _,
            size_of::<MEMORY_BASIC_INFORMATION>()
        );
        //Couldnt get mapping size
        if ret_val == 0 {
            UnmapViewOfFile(map_addr);
            CloseHandle(map_handle);
            return Err(From::from(format!("VirtualQuery failed with {}", GetLastError())));
        }

        mem_ba.RegionSize
    };

    //Figure out what the lock type is based on the shared_data set by create()
    let shared_data: &SharedData = unsafe {&(*(map_addr as *mut SharedData))};
    let lock_ind = shared_data.lock_ind;
    let lock_type: LockType = ind_to_locktype(&(lock_ind as usize));

    //Ensure our shared data is 4 byte aligned
    let shared_data_sz = (size_of::<SharedData>() + 3) & !(0x03 as usize);
    let lock_data_sz = get_supported_lock_size(&lock_type);

    //Use the proper lock type implementation
    let meta: MemMetadata = MemMetadata {
        //map_name: mapping_path,
        map_handle: map_handle,
        //map_size: full_size,
        shared_data: map_addr as *mut SharedData,
        lock_data: (map_addr as usize + shared_data_sz) as *mut _,
        data: (map_addr as usize + shared_data_sz + lock_data_sz) as *mut c_void,
        lock_impl: get_supported_lock(&lock_type),//get_supported_lock(&lock_type),
    };

    //Pick the smallest size
    if full_size > shared_data.mapping_size {
        new_file.size = shared_data.mapping_size;
    } else {
        new_file.size = full_size - shared_data_sz - lock_data_sz;
    }

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

    //Get the total size with all the added metadata
    let shared_data_sz = (size_of::<SharedData>() + 3) & !(0x03 as usize);
    let lock_data_sz: usize = get_supported_lock_size(&lock_type);
    let actual_size: usize = new_file.size + lock_data_sz + shared_data_sz;

    //Create mapping and map to our address space
    let map_handle = unsafe {
        let high_size: u32 = (actual_size as u64 & 0xFFFFFFFF00000000 as u64) as u32;
        let low_size: u32 = (actual_size as u64 & 0xFFFFFFFF as u64) as u32;
        CreateFileMappingA(
            INVALID_HANDLE_VALUE,
            null_mut(),
            PAGE_READWRITE,
            high_size,
            low_size,
            CString::new(real_path.clone())?.as_ptr())
    };
    if map_handle == NULL {
        return Err(From::from(format!("CreateFileMappingA failed with {}", unsafe{GetLastError()})));
    }

    new_file.real_path = Some(real_path.clone());

    let map_addr = unsafe {
        MapViewOfFile(
            map_handle,
            FILE_MAP_READ| FILE_MAP_WRITE,
            0,
            0,
            0
        )
    };
    if map_addr == NULL {
        unsafe { CloseHandle(map_handle); }
        return Err(From::from(format!("MapViewOfFile failed with {}", unsafe{GetLastError()})));
    }

    let meta = MemMetadata {
        //map_name: real_path,
        map_handle: map_handle,
        //map_size: actual_size,
        shared_data: map_addr as *mut SharedData,
        lock_data: (map_addr as usize + shared_data_sz) as *mut _,
        data: (map_addr as usize + shared_data_sz + lock_data_sz) as *mut c_void,
        lock_impl: get_supported_lock(&lock_type),
    };

    let shared_data: &mut SharedData = unsafe {
        &mut (*meta.shared_data)
    };

    //Set the lock type and mapping size
    shared_data.lock_ind = locktype_to_ind(&lock_type) as u8;
    shared_data.mapping_size = new_file.size;

    new_file.meta = Some(meta);

    Ok(new_file)
}

//Returns the size we need to allocate in the shared memory for our lock
fn get_supported_lock_size(lock_type: &LockType) -> usize {
    match lock_type {
        &LockType::None => LockNone::size_of(),
        //&LockType::Rwlock => RwLock::size_of(),
        _ => unimplemented!("Windows does not support this lock type..."),
    }
}

//Returns a boxed trait that implements MemFileLockImpl for the specified type
fn get_supported_lock(lock_type: &LockType) -> &'static MemFileLockImpl {
    match lock_type {
        &LockType::None => &LockNone{},
        //&LockType::Rwlock => &RwLock{},
        _ => unimplemented!("Windows does not support this lock type..."),
    }
}
