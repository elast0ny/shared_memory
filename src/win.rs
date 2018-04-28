extern crate winapi;

use self::winapi::shared::ntdef::{NULL};
use self::winapi::shared::minwindef::{FALSE};
use self::winapi::shared::winerror::*;
use self::winapi::um::winbase::*;
use self::winapi::um::winnt::*;
use self::winapi::um::handleapi::*;
use self::winapi::um::memoryapi::*;
use self::winapi::um::errhandlingapi::*;

use self::winapi::um::synchapi::{
    CreateMutexA,
    //OpenMutexA, //This is in winbase ??
    WaitForSingleObject,
    ReleaseMutex,
    //WaitForMultipleObjects,
};

use super::{std,
    SharedMem,
    LockType,
    LockNone,
    SharedMemLockImpl,
};

use std::path::PathBuf;
use std::mem::size_of;
use std::ffi::CString;
use std::ptr::{null_mut};
use std::os::raw::c_void;

use std::slice;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

///Struct that will be located in the shared memory
struct SharedData {
    //This field is used to transmit the locking mechanism to an openner
    lock_ind: u8,
    //We can provide a more precise mapping size through this. Windows
    //rounds up to PAGE_SIZE when using VirtualQuery
    mapping_size: usize,
}

///This struct describes our memory mapping
pub struct MemMetadata<'a> {

    /* Optionnal implementation fields */

    ///The handle to our open mapping
    map_handle: HANDLE,
    ///Hold data to control the mapping (locks)
    shared_data: *mut SharedData,

    /* Mandatory fields */
    ///the shared memory for our lock
    pub lock_data: *mut c_void,
    //Set this to true when our lock_data contains a handle
    pub lock_data_is_handle: bool,
    ///Pointer to user data
    pub data: *mut c_void,
    //Our custom lock implementation
    pub lock_impl : &'a SharedMemLockImpl,
}

///Teardown UnmapViewOfFile and close CreateMapping handle
impl<'a> Drop for MemMetadata<'a> {
    ///Takes care of properly closing the SharedMem (munmap(), shmem_unlink(), close())
    fn drop(&mut self) {
        //If we have an open lock handle
        if self.lock_data_is_handle {
            unsafe { CloseHandle(self.lock_data as *mut _); }
        }
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

//Opens an existing SharedMem, OpenFileMappingA()/MapViewOfFile()/VirtualQuery()
pub fn open(mut new_file: SharedMem) -> Result<SharedMem> {

    //If there is a link file, this isnt a raw mapping
    let is_raw: bool = !new_file.link_path.is_some();

    // Get the shmem path
    let mapping_path = match new_file.real_path {
        Some(ref path) => path.clone(),
        None => {
            panic!("Tried to open SharedMem with no real_path");
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

    //Do not not add any meta_data locking if raw mapping
    if is_raw {
        //We cannot get a more precise size than what VirtualQuery is telling us
        new_file.size = full_size;
        new_file.meta = Some(MemMetadata {
            map_handle: map_handle,
            shared_data: map_addr as *mut SharedData,
            lock_data: null_mut(),
            lock_data_is_handle: false,
            data: map_addr as *mut c_void,
            lock_impl: &LockNone{},
        });

        return Ok(new_file);
    }

    //Figure out what the lock type is based on the shared_data set by create()
    let shared_data: &SharedData = unsafe {&(*(map_addr as *mut SharedData))};
    let lock_info = supported_locktype_from_ind(shared_data.lock_ind as usize);
    let lock_type: LockType = lock_info.0;

    //Ensure our shared data is 4 byte aligned
    let shared_data_sz = (size_of::<SharedData>() + 3) & !(0x03 as usize);
    let lock_data_sz = lock_info.1;

    //Use the proper lock type implementation
    let mut meta: MemMetadata = MemMetadata {
        map_handle: map_handle,
        shared_data: map_addr as *mut SharedData,
        lock_data: (map_addr as usize + shared_data_sz) as *mut _,
        lock_data_is_handle: false,
        data: (map_addr as usize + shared_data_sz + lock_data_sz) as *mut c_void,
        lock_impl: &LockNone{},
    };

    match lock_type {
        LockType::None => {},
        LockType::Mutex => {
            //Grab mutex namespace from shared memory
            let mut mutex_name: String = String::with_capacity(Mutex::size_of());
            for char_byte in unsafe {slice::from_raw_parts((meta.lock_data) as *const u8, Mutex::size_of())} {
                if *char_byte == 0x00 { break }
                mutex_name.push(*char_byte as char);
            }

            meta.lock_data = unsafe {OpenMutexA(
                SYNCHRONIZE,            // request full access
                FALSE,                       // handle not inheritable
                CString::new(mutex_name)?.as_ptr()) as *mut _};

            if meta.lock_data as *mut winapi::ctypes::c_void == NULL {
                return Err(From::from(format!("OpenMutexA failed with {}", unsafe{GetLastError()})));
            }
            meta.lock_data_is_handle = true;
            meta.lock_impl = &Mutex{};
        },
        LockType::RwLock => {
        }
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

//Creates a new SharedMem, CreateFileMappingA()/MapViewOfFile()
pub fn create(mut new_file: SharedMem, lock_type: LockType) -> Result<SharedMem> {

    let max_path_len = 260;

    // real_path is either :
    // 1. Specified directly
    // 2. Needs to be generated (link_file needs to exist)
    let is_raw: bool;
    let mut real_path: String = match new_file.real_path {
        Some(ref path) => {
            is_raw = true;
            path.clone()
        },
        None => {
            is_raw = false;
            //We dont have a real path and a link file wasn created
            if let Some(ref file_path) = new_file.link_path {
                if !file_path.is_file() {
                    return Err(From::from("os_impl::create() on a link but not link file exists..."));
                }

                //Get unique name for shmem object
                let abs_disk_path: PathBuf = file_path.canonicalize()?;
                let mut chars: &str = &abs_disk_path.to_string_lossy();

                //Make sure we generate a path that isnt too long
                let str_len: usize = chars.len();
                if str_len > max_path_len {
                    chars = &chars[str_len-max_path_len..str_len];
                }

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
                String::from(unique_name.trim_matches('_'))
            } else {
                //lib.rs shouldnt call us without either real_path or link_path set
                panic!("Trying to create SharedMem without any name");
            }
        }
    };

    //Make sure we support this LockType
    let locktype_info = supported_locktype_info(&lock_type);

    let mut shared_data_sz: usize = 0;
    let mut lock_ind: u8 = 0;
    let mut lock_data_sz: usize = 0;

    //If not raw, add our SharedMem metadata
    if !is_raw {
        //Get the total size with all the added metadata
        shared_data_sz = (size_of::<SharedData>() + 3) & !(0x03 as usize);
        lock_ind = locktype_info.0 as u8;
        lock_data_sz = locktype_info.1;
    }

    let actual_size: usize = new_file.size + lock_data_sz + shared_data_sz;
    let mut map_handle = NULL;
    let mut retry: usize = 0;
    let mut orig_path: String = String::with_capacity(real_path.len() + 4);

    while map_handle == NULL {
        //Create mapping and map to our address space
        map_handle = unsafe {
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
        let last_error = unsafe{GetLastError()};
        if map_handle == NULL {
            return Err(From::from(format!("CreateFileMappingA failed with {}", last_error)));
        } else if last_error == ERROR_ALREADY_EXISTS {
            //CreateFileMapping returned a handle to the existing mapping
            unsafe { CloseHandle(map_handle)};
            map_handle = NULL;

            if retry == 0 {
                 orig_path = real_path.clone();
            }
            real_path = format!("{}_{}", orig_path, retry);
            retry += 1;

            //Make sure we generated a path that isnt too long
            let str_len: usize = real_path.len();
            if str_len > max_path_len {
                real_path = real_path[str_len-max_path_len..str_len].to_string();
            }

            println!("Mapping with same name already exists ! Trying another name \"{}\"", real_path);
            continue;
        }
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

    //Nothing else to do if raw mapping
    if is_raw {
        new_file.meta = Some(MemMetadata {
            map_handle: map_handle,
            shared_data: map_addr as *mut SharedData,
            lock_data: null_mut(),
            lock_data_is_handle: false,
            data: map_addr as *mut c_void,
            lock_impl: &LockNone{},
        });

        return Ok(new_file);
    }

    /* Init shared memory meta data */

    let mut meta: MemMetadata = MemMetadata {
        map_handle: map_handle,
        shared_data: map_addr as *mut SharedData,
        data: (map_addr as usize + shared_data_sz + lock_data_sz) as *mut c_void,

        lock_data: (map_addr as usize + shared_data_sz) as *mut _,
        lock_data_is_handle: false,
        lock_impl: &LockNone{},
    };

    //Init our shared metadata
    let shared_data: &mut SharedData = unsafe {
        &mut (*meta.shared_data)
    };
    shared_data.mapping_size = new_file.size;
    shared_data.lock_ind = lock_ind;

    //Init lock
    match lock_type {
        LockType::None => {},
        LockType::Mutex => {
            //Write mutex name to shared memory
            let mutex_path: String = String::from("test_mutex");
            let lock_data_as_slice: &mut [u8] = unsafe {
                slice::from_raw_parts_mut((meta.lock_data) as *mut u8, Mutex::size_of())
            };
            lock_data_as_slice[0..mutex_path.as_bytes().len()].copy_from_slice(mutex_path.as_bytes());

            //Our lock_data ptr now holds a handle
            meta.lock_data = unsafe {CreateMutexA(
                null_mut(),              // default security attributes
                FALSE,             // initially not owned
                CString::new(mutex_path)?.as_ptr()) as *mut _};
            if meta.lock_data as *mut winapi::ctypes::c_void == NULL {
                return Err(From::from(format!("CreateMutexA failed with {}", unsafe{GetLastError()})));
            }
            meta.lock_data_is_handle = true;
            meta.lock_impl = &Mutex{};
        },
        LockType::RwLock => {
        }
    };

    new_file.meta = Some(meta);
    Ok(new_file)
}

//Returns the index and size of the lock_type
fn supported_locktype_info(lock_type: &LockType) -> (usize, usize) {
    match lock_type {
        &LockType::None => (0, LockNone::size_of()),
        &LockType::Mutex => (1, Mutex::size_of()),
        //&LockType::RwLock => (2, RwLock::size_of()),
        _ => unimplemented!("Windows does not support this lock type..."),
    }
}

//Returns the proper locktype and size of the structure
fn supported_locktype_from_ind(index: usize) -> (LockType, usize) {
    match index {
        0 => (LockType::None, LockNone::size_of()),
        1 => (LockType::Mutex, Mutex::size_of()),
        //2 => (LockType::RwLock, RwLock::size_of()),
        _ => unimplemented!("Windows does not support this locktype index..."),
    }
}

/* Lock Implementations */


//Mutex
pub struct Mutex {}
impl Mutex {
    pub fn acquire_lock(&self, handle: *mut winapi::ctypes::c_void) -> Result<()> {
        //Wait for mutex to be availabe
        let wait_res = unsafe {WaitForSingleObject(
            handle,
            INFINITE)};

        if wait_res == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err(From::from("Failed to acquire Mutex !"))
        }
    }
    pub fn release_lock(&self, handle: *mut winapi::ctypes::c_void) {
        unsafe {ReleaseMutex(handle)};
    }
}
impl SharedMemLockImpl for Mutex {

    fn size_of() -> usize {
        //A mutex is identified by a Windows namespace with a max of 255 characters
        255
    }
    //Both rlock and wlock are the same for Mutexes
    fn rlock(&self, lock_data: *mut c_void) -> Result<()> {
        self.acquire_lock(lock_data as *mut winapi::ctypes::c_void)
    }
    fn wlock(&self, lock_data: *mut c_void) -> Result<()> {
        self.acquire_lock(lock_data as *mut winapi::ctypes::c_void)
    }
    fn runlock(&self, lock_data: *mut c_void) -> () {
        self.release_lock(lock_data as *mut winapi::ctypes::c_void);
    }
    fn wunlock(&self, lock_data: *mut c_void) -> () {
        self.release_lock(lock_data as *mut winapi::ctypes::c_void);
    }
}
