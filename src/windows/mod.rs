use ::rand::Rng;

use ::winapi::shared::ntdef::{NULL};
use ::winapi::shared::minwindef::{FALSE};
use ::winapi::shared::winerror::*;
use ::winapi::um::winbase::{
    CreateFileMappingA,
    OpenFileMappingA,
    INFINITE,
    WAIT_OBJECT_0,
    OpenMutexA,
};
use ::winapi::um::winnt::*;
use ::winapi::um::handleapi::*;
use ::winapi::um::memoryapi::*;
use ::winapi::um::errhandlingapi::*;

use ::winapi::um::synchapi::{
    CreateMutexA,
    //OpenMutexA, //This is in winbase ??
    WaitForSingleObject,
    ReleaseMutex,
    //WaitForMultipleObjects,
    CreateEventExA,
    CREATE_EVENT_MANUAL_RESET,
    OpenEventA,
    SetEvent,
    ResetEvent,
};

use crate::{
    SharedMemError,
    LockType,
    GenericLock,
    LockImpl,
    EventType,
    EventImpl,
    EventState,
    Timeout,
    GenericEvent,
    AutoBusy,
    ManualBusy,
};

use std::mem::size_of;
use std::ffi::CString;
use std::ptr::{null_mut};
use std::os::raw::c_void;

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
        //Unmap memory from our process
        if self.map_ptr as *mut _ != NULL {
            unsafe { UnmapViewOfFile(self.map_ptr as *mut _); }
        }

        //Close our mapping
        if self.map_handle as *mut _ != NULL {
            unsafe { CloseHandle(self.map_handle); }
        }
    }
}

//Creates a mapping specified by the uid and size
pub fn create_mapping(unique_id: &str, map_size: usize) -> Result<MapData, SharedMemError> {

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
            CString::new(unique_id).unwrap().as_ptr())
    };
    let last_error = unsafe{GetLastError()};

    if new_map.map_handle == NULL {
        return Err(SharedMemError::MapCreateFailed(last_error));
    } else if last_error == ERROR_ALREADY_EXISTS {
        return Err(SharedMemError::MappingIdExists);
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
        let last_error = unsafe{GetLastError()};
        unsafe { CloseHandle(new_map.map_handle); }
        return Err(SharedMemError::MapCreateFailed(last_error));
    }

    Ok(new_map)
}

//Opens an existing mapping specified by its uid
pub fn open_mapping(unique_id: &str) -> Result<MapData, SharedMemError> {

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
           #[allow(clippy::temporary_cstring_as_ptr)]
           CString::new(unique_id).unwrap().as_ptr()
       )
   };
   if new_map.map_handle as *mut _ == NULL {
       let last_error = unsafe{GetLastError()};
       return Err(SharedMemError::MapOpenFailed(last_error));
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
        let last_error = unsafe{GetLastError()};
        return Err(SharedMemError::MapOpenFailed(last_error));
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
            let last_error = GetLastError();
            return Err(SharedMemError::UnknownOsError(last_error));
        }

        mem_ba.RegionSize
    };

    Ok(new_map)
}

//This functions exports our implementation for each lock type
pub fn lockimpl_from_type(lock_type: LockType) -> &'static dyn LockImpl {
    match lock_type {
        LockType::Mutex => &Mutex{},
        LockType::RwLock => unimplemented!("shared_memory does not have a RwLock implementation for Windows..."),
    }
}

//This functions exports our implementation for each event type
pub fn eventimpl_from_type(event_type: EventType) -> &'static dyn EventImpl {
    match event_type {
        EventType::AutoBusy => &AutoBusy{},
        EventType::ManualBusy => &ManualBusy{},
        EventType::Manual => &ManualGeneric{},
        EventType::Auto => &AutoGeneric{},
    }
}
//This struct holds a unique ID which is used for the Windows Object's namespace
struct FeatureId {
    id: u32,
}
impl FeatureId {
    pub fn get_namespace(&self) -> String {
        format!("shmem_rs_{:8X}", self.id)
    }
}

/* Lock Implementations */

//Mutex

fn acquire_mutex(handle: *mut winapi::ctypes::c_void) -> Result<(), SharedMemError> {
    let wait_res = unsafe {WaitForSingleObject(
        handle,
        INFINITE)};

    if wait_res == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err(SharedMemError::FailedToAcquireLock(wait_res))
    }
}

pub struct Mutex {}
impl LockImpl for Mutex {

    fn size_of(&self) -> usize {
        size_of::<FeatureId>()
    }
    fn init(&self, lock_info: &mut GenericLock, create_new: bool) -> Result<(), SharedMemError> {

        let unique_id: &mut FeatureId = unsafe {&mut (*(lock_info.lock_ptr as *mut FeatureId))};

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
                    #[allow(clippy::temporary_cstring_as_ptr)]
                    CString::new(unique_id.get_namespace()).unwrap().as_ptr()) as *mut _
                };
                let last_error = unsafe{GetLastError()};

                if lock_info.lock_ptr as *mut _ == NULL {
                    return Err(SharedMemError::FailedToCreateLock(last_error));
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
                return Err(SharedMemError::FailedToCreateLock(0));
            }

            lock_info.lock_ptr = unsafe {
                OpenMutexA(
                    SYNCHRONIZE,    // request full access
                    FALSE,          // handle not inheritable
                    #[allow(clippy::temporary_cstring_as_ptr)]
                    CString::new(unique_id.get_namespace()).unwrap().as_ptr()
                ) as *mut _
            };

            if lock_info.lock_ptr as *mut _ == NULL {
                let last_error = unsafe{GetLastError()};
                return Err(SharedMemError::FailedToCreateLock(last_error));
            }
        }

        Ok(())
    }
    fn destroy(&self, lock_info: &mut GenericLock) {
        unsafe {CloseHandle(lock_info.lock_ptr)};
    }
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<(), SharedMemError> {
        acquire_mutex(lock_ptr)
    }
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<(), SharedMemError> {
        acquire_mutex(lock_ptr)
    }
    fn runlock(&self, lock_ptr: *mut c_void) {
        unsafe {ReleaseMutex(lock_ptr)};
    }
    fn wunlock(&self, lock_ptr: *mut c_void) {
        unsafe {ReleaseMutex(lock_ptr)};
    }
}

/* Event implementations */

fn timeout_to_milli(timeout: &Timeout) -> u32 {
    match *timeout {
        Timeout::Infinite => INFINITE,
        Timeout::Sec(t) => (t * 1_000) as u32,
        Timeout::Milli(t) => (t) as u32,
        Timeout::Micro(t) => (t / 1_000) as u32,
        Timeout::Nano(t) => (t / 1_000_000) as u32,
    }
}

fn event_init(event_info: &mut GenericEvent, create_new: bool, manual_reset: bool) -> Result<(), SharedMemError> {
    let unique_id: &mut FeatureId = unsafe {&mut (*(event_info.ptr as *mut FeatureId))};

    //Create the mutex and set the ID
    if create_new {
        unique_id.id = 0;
        event_info.ptr = NULL;

        loop {
            while unique_id.id == 0 {
                unique_id.id = rand::thread_rng().gen::<u32>();
            }

            event_info.ptr = unsafe {
                CreateEventExA(
                    null_mut(),
                    #[allow(clippy::temporary_cstring_as_ptr)]
                    CString::new(unique_id.get_namespace()).unwrap().as_ptr(),
                    if manual_reset {
                        CREATE_EVENT_MANUAL_RESET
                    } else {
                        0
                    },
                    EVENT_MODIFY_STATE | SYNCHRONIZE,
                ) as *mut _
            };
            let last_error = unsafe{GetLastError()};

            if event_info.ptr as *mut _ == NULL {
                return Err(SharedMemError::FailedToCreateEvent(last_error));
            } else if last_error == ERROR_ALREADY_EXISTS {
                //Generate another ID and try again
                unsafe {CloseHandle(event_info.ptr)};
                continue;
            }

            //No error, we have create a mutex !
            break;
        }

    } else {
        if unique_id.id == 0 {
            return Err(SharedMemError::FailedToCreateEvent(0));
        }

        event_info.ptr = unsafe {
            OpenEventA(
                EVENT_MODIFY_STATE | SYNCHRONIZE,    // request full access
                FALSE,          // handle not inheritable
                #[allow(clippy::temporary_cstring_as_ptr)]
                CString::new(unique_id.get_namespace()).unwrap().as_ptr()
            ) as *mut _
        };

        if event_info.ptr as *mut _ == NULL {
            let last_error = unsafe{GetLastError()};
            return Err(SharedMemError::FailedToCreateEvent(last_error));
        }
    }

    Ok(())
}

pub struct AutoGeneric {}
impl EventImpl for AutoGeneric {
    ///Returns the size of the event structure that will live in shared memory
    fn size_of(&self) -> usize {
        // + 3 allows us to move our EventCond to align it in the shmem
        size_of::<FeatureId>()
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<(), SharedMemError> {
        event_init(event_info, create_new, false)
    }
    ///De-initializes the event
    fn destroy(&self, event_info: &mut GenericEvent) {
        unsafe {CloseHandle(event_info.ptr)};
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<(), SharedMemError> {
        let wait_res = unsafe {
            WaitForSingleObject(
                event_ptr,
                timeout_to_milli(&timeout)
            )
        };

        if wait_res == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err(SharedMemError::Timeout)
        }
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<(), SharedMemError> {
        if match state {
            EventState::Wait => unsafe {ResetEvent(event_ptr)},
            EventState::Signaled => unsafe {SetEvent(event_ptr)}
        } == 0 {
            let last_error = unsafe{GetLastError()};
            return Err(SharedMemError::FailedToSignalEvent(last_error))
        }

        Ok(())
    }
}

pub struct ManualGeneric {}
impl EventImpl for ManualGeneric {
    ///Returns the size of the event structure that will live in shared memory
    fn size_of(&self) -> usize {
        // + 3 allows us to move our EventCond to align it in the shmem
        size_of::<FeatureId>()
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<(), SharedMemError> {
        event_init(event_info, create_new, true)
    }
    ///De-initializes the event
    fn destroy(&self, event_info: &mut GenericEvent) {
        unsafe {CloseHandle(event_info.ptr)};
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<(), SharedMemError> {
        let wait_res = unsafe {
            WaitForSingleObject(
                event_ptr,
                timeout_to_milli(&timeout)
            )
        };

        if wait_res == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err(SharedMemError::Timeout)
        }
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<(), SharedMemError> {
        if match state {
            EventState::Wait => unsafe {ResetEvent(event_ptr)},
            EventState::Signaled => unsafe {SetEvent(event_ptr)}
        } == 0 {
            let last_error = unsafe{GetLastError()};
            return Err(SharedMemError::FailedToCreateEvent(last_error))
        }

        Ok(())
    }
}
