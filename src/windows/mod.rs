extern crate winapi;

extern crate rand;
use self::rand::Rng;

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
    LockType,
    GenericLock,
    LockImpl,
    EventType,
    EventImpl,
    EventState,
    Timeout,
    GenericEvent,
    Result,
};

use std::path::PathBuf;
use std::mem::size_of;
use std::ffi::CString;
use std::ptr::{null_mut};
use std::os::raw::c_void;
use std::slice;

pub struct MapData {

    ///The handle to our open mapping
    map_handle: HANDLE,

    //Shared mapping uid
    pub unique_id: String,
    //Total size of the mapping
    pub map_size: usize,
    //Pointer to the first address of our mapping
    pub map_ptr: *mut c_void,
}

///Teardown UnmapViewOfFile and close CreateMapping handle
impl Drop for MapData {
    ///Takes care of properly closing the SharedMem (munmap(), shmem_unlink(), close())
    fn drop(&mut self) {
        /*
        //If we have an open lock handle
        if self.lock_data_is_handle {
            unsafe { CloseHandle(self.lock_data as *mut _); }
        }
        */
        //Unmap memory from our process
        if self.map_ptr as *mut _ == NULL {
            unsafe { UnmapViewOfFile(self.map_ptr as *mut _); }
        }

        //Close our mapping
        if self.map_handle as *mut _ != NULL {
            unsafe { CloseHandle(self.map_handle); }
        }
    }
}

//Creates a mapping specified by the uid and size
pub fn create_mapping(unique_id: &str, map_size: usize) -> Result<MapData> {

    let mut new_map: MapData = MapData {
        unique_id: String::from(unique_id),
        map_handle: NULL,
        map_size: map_size,
        map_ptr: null_mut(),
    };

    //Create Mapping
    new_map.map_handle = unsafe {
        let high_size: u32 = (map_size as u64 & 0xFFFFFFFF00000000 as u64) as u32;
        let low_size: u32 = (map_size as u64 & 0xFFFFFFFF as u64) as u32;
        CreateFileMappingA(
            INVALID_HANDLE_VALUE,
            null_mut(),
            PAGE_READWRITE,
            high_size,
            low_size,
            CString::new(unique_id)?.as_ptr())
    };
    let last_error = unsafe{GetLastError()};

    if new_map.map_handle == NULL {
        return Err(From::from(format!("CreateFileMappingA failed with {}", last_error)));
    } else if last_error == ERROR_ALREADY_EXISTS {
        return Err(From::from("NAME_EXISTS"));
    }

    //Map mapping into address space
    new_map.map_ptr = unsafe {
        MapViewOfFile(
            new_map.map_handle,
            FILE_MAP_READ| FILE_MAP_WRITE,
            0,
            0,
            0
        )
    };
    if new_map.map_ptr == NULL {
        unsafe { CloseHandle(new_map.map_handle); }
        return Err(From::from(format!("MapViewOfFile failed with {}", unsafe{GetLastError()})));
    }

    Ok(new_map)
}

//Opens an existing mapping specified by its uid
pub fn open_mapping(unique_id: &str) -> Result<MapData> {

    let mut new_map: MapData = MapData {
        unique_id: String::from(unique_id),
        map_handle: NULL,
        map_size: 0,
        map_ptr: null_mut(),
    };

    //Open existing mapping
    new_map.map_handle = unsafe {
       OpenFileMappingA(
           FILE_MAP_READ| FILE_MAP_WRITE,
           FALSE,
           CString::new(unique_id)?.as_ptr()
       )
   };
   if new_map.map_handle as *mut _ == NULL {
       return Err(From::from(format!("OpenFileMappingA failed with {}", unsafe{GetLastError()})));
   }

   //Map mapping into address space
   new_map.map_ptr = unsafe {
        MapViewOfFile(
            new_map.map_handle,
            FILE_MAP_READ| FILE_MAP_WRITE,
            0,
            0,
            0
        )
    };
    if new_map.map_ptr == NULL {
        return Err(From::from(format!("MapViewOfFile failed with {}", unsafe{GetLastError()})));
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
            size_of::<MEMORY_BASIC_INFORMATION>()
        );

        //Couldnt get mapping size
        if ret_val == 0 {
            return Err(From::from(format!("VirtualQuery failed with {}", GetLastError())));
        }

        mem_ba.RegionSize
    };

    Ok(new_map)
}

//This functions exports our implementation for each lock type
pub fn lockimpl_from_type(lock_type: &LockType) -> &'static LockImpl {
    match lock_type {
        &LockType::Mutex => &Mutex{},
        &LockType::RwLock => unimplemented!("shared_memory does not have a RwLock implementation for Windows..."),
    }
}

//This functions exports our implementation for each event type
pub fn eventimpl_from_type(event_type: &EventType) -> &'static EventImpl {
    match event_type {
        &EventType::AutoBusy => unimplemented!("shared_memory does not have a AutoBusy implementation for Windows..."),
        &EventType::ManualBusy => unimplemented!("shared_memory does not have a ManualBusy implementation for Windows..."),
        &EventType::Manual => unimplemented!("shared_memory does not have a Manual implementation for Windows..."),
        &EventType::Auto => unimplemented!("shared_memory does not have a Auto implementation for Windows..."),
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

struct MutexId {
    id: u32,
}

impl MutexId {
    pub fn get_namespace(&self) -> String {
        format!("shmem_rs_mutex_{:8X}", self.id)
    }
}

fn acquire_mutex(handle: *mut winapi::ctypes::c_void) -> Result<()> {
    let wait_res = unsafe {WaitForSingleObject(
        handle,
        INFINITE)};

    if wait_res == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err(From::from("Failed to acquire Mutex !"))
    }
}

impl LockImpl for Mutex {

    fn size_of(&self) -> usize {
        size_of::<MutexId>()
    }
    fn init(&self, lock_info: &mut GenericLock, create_new: bool) -> Result<()> {

        let unique_id: &mut MutexId = unsafe {&mut (*(lock_info.lock_ptr as *mut MutexId))};

        //Create the mutex and set the ID
        if create_new {
            unique_id.id = 0;
            lock_info.lock_ptr = NULL;

            loop {
                while unique_id.id == 0 {
                    unique_id.id = rand::thread_rng().gen::<u32>();
                }

                lock_info.lock_ptr = unsafe {
                    CreateMutexA(
                    null_mut(),              // default security attributes
                    FALSE,             // initially not owned
                    CString::new(unique_id.get_namespace())?.as_ptr()) as *mut _
                };
                let last_error = unsafe{GetLastError()};

                if lock_info.lock_ptr as *mut _ == NULL {
                    return Err(From::from(format!("[Create|Open]MutexA failed with {}", unsafe{GetLastError()})));
                } else if last_error == ERROR_ALREADY_EXISTS {
                    //Generate another ID and try again
                    unsafe {CloseHandle(lock_info.lock_ptr)};
                    continue;
                }

                //No error, we have create a mutex !
                break;
            }

        } else {
            if unique_id.id == 0 {
                return Err(From::from("Mutex.init() [OPEN] : Mutex_id is 0... Has it been properly created ?"));
            }

            lock_info.lock_ptr = unsafe {
                OpenMutexA(
                    SYNCHRONIZE,    // request full access
                    FALSE,          // handle not inheritable
                    CString::new(unique_id.get_namespace())?.as_ptr()
                ) as *mut _
            };

            if lock_info.lock_ptr as *mut _ == NULL {
                return Err(From::from(format!("[Create|Open]MutexA failed with {}", unsafe{GetLastError()})));
            }
        }

        Ok(())
    }
    fn destroy(&self, lock_info: &mut GenericLock) {
        unsafe {CloseHandle(lock_info.lock_ptr)};
    }
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        acquire_mutex(lock_ptr)
    }
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        acquire_mutex(lock_ptr)
    }
    fn runlock(&self, lock_ptr: *mut c_void) {
        unsafe {ReleaseMutex(lock_ptr)};
    }
    fn wunlock(&self, lock_ptr: *mut c_void) {
        unsafe {ReleaseMutex(lock_ptr)};
    }
}
