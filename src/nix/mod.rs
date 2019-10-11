use ::cfg_if::*;

cfg_if!{
    if #[cfg(target_os="linux")] {
        pub mod linux;
        pub use crate::nix::linux::*;
        use ::libc::pthread_mutex_timedlock;
    } else if #[cfg(target_os="macos")] {
        pub mod mac;
        pub use crate::nix::mac::*;
    } else {
        use ::libc::pthread_mutex_timedlock;
    }
}

use ::libc::{
    timespec,
    time_t,
    c_long,
    clock_gettime,
    CLOCK_REALTIME,

    //Mutex defs
    pthread_mutex_t,
    pthread_mutex_init,
    pthread_mutex_lock,
    pthread_mutex_unlock,
    //Mutex attribute
    pthread_mutexattr_t,
    pthread_mutexattr_init,
    pthread_mutexattr_setpshared,

    //Rwlock defs
    pthread_rwlock_t,
    pthread_rwlock_init,
    pthread_rwlock_unlock,
    pthread_rwlock_rdlock,
    pthread_rwlock_wrlock,
    //RW Atribute
    pthread_rwlockattr_t,
    pthread_rwlockattr_init,
    pthread_rwlockattr_setpshared,

    //Events
    pthread_cond_t,
    pthread_cond_init,
    pthread_cond_wait,
    pthread_condattr_t,
    pthread_cond_signal,
    pthread_condattr_init,
    pthread_cond_broadcast,
    pthread_cond_timedwait,
    pthread_condattr_setpshared,

    PTHREAD_PROCESS_SHARED,
};

use ::nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, ProtFlags, MapFlags};
use ::nix::errno::Errno;
use ::nix::sys::stat::{fstat, Mode};
use ::nix::fcntl::OFlag;
use ::nix::unistd::{close, ftruncate};

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

use std::os::raw::c_void;
use std::os::unix::io::RawFd;
use std::ptr::{null_mut};
use std::mem::size_of;

/*
#[cfg(target_os="macos")]
pub const MAX_NAME:usize = 30;
#[cfg(any(target_os="freebsd", target_os="linux"))]
pub const MAX_NAME:usize = 255;
*/

pub struct MapData {

    //On linux, you must shm_unlink() the object created for the mapping. It wont disappear automatically.
    owner: bool,
    pid: i32,

    //File descriptor to our open mapping
    map_fd: RawFd,

    //Shared mapping uid
    pub unique_id: String,
    //Total size of the mapping
    pub map_size: usize,
    //Pointer to the first address of our mapping
    pub map_ptr: *mut c_void,
}

///shared memory teardown for linux
impl Drop for MapData {
    ///Takes care of properly closing the SharedMem (munmap(), shmem_unlink(), close())
    fn drop(&mut self) {

        //Unmap memory
        if !self.map_ptr.is_null() {
            match unsafe {munmap(self.map_ptr as *mut _, self.map_size)} {
                Ok(_) => {},
                Err(_e) => {
                    //debug!("os_impl::Linux : Failed to munmap() shared memory mapping...");
                    //debug!("{}", e);
                },
            };
        }

        //Unlink shmem
        if self.map_fd != 0 {
            //unlink shmem if we created it
            if self.owner && self.pid == unsafe { libc::getpid() } {
                match shm_unlink(self.unique_id.as_str()) {
                    Ok(_) => {
                        //debug!("shm_unlink()");
                    },
                    Err(_e) => {
                        //debug!("os_impl::Linux : Failed to shm_unlink() shared memory name...");
                        //debug!("{}", e);
                    },
                };
            }

            match close(self.map_fd) {
                Ok(_) => {
                    //debug!("close()");
                },
                Err(_e) => {
                    //debug!("os_impl::Linux : Failed to close() shared memory file descriptor...");
                    //debug!("{}", e);
                },
            };
        }
    }
}

//Creates a mapping specified by the uid and size
pub fn create_mapping(unique_id: &str, map_size: usize) -> Result<MapData, SharedMemError> {

    //Create shared memory file descriptor
    let shmem_fd = match shm_open(
        unique_id, //Unique name that usualy pops up in /dev/shm/
        OFlag::O_CREAT|OFlag::O_EXCL|OFlag::O_RDWR, //create exclusively (error if collision) and read/write to allow resize
        Mode::S_IRUSR|Mode::S_IWUSR //Permission allow user+rw
    ) {
        Ok(v) => v,
        Err(nix::Error::Sys(Errno::EEXIST)) => return Err(SharedMemError::MappingIdExists),
        Err(nix::Error::Sys(e)) => return Err(SharedMemError::MapCreateFailed(e as u32)),
        _ => return Err(SharedMemError::UnknownOsError(0xffff_ffff)),
    };

    let mut new_map: MapData = MapData {
        owner: true,
        pid: unsafe { libc::getpid() },
        unique_id: String::from(unique_id),
        map_fd: shmem_fd,
        map_size: map_size,
        map_ptr: null_mut(),
    };

    //Enlarge the memory descriptor file size to the requested map size
    match ftruncate(new_map.map_fd, new_map.map_size as _) {
        Ok(_) => {},
        Err(nix::Error::Sys(e)) => return Err(SharedMemError::UnknownOsError(e as u32)),
        _ => return Err(SharedMemError::UnknownOsError(0xffff_ffff)),
    };

    //Put the mapping in our address space
    new_map.map_ptr = match unsafe {
        mmap(null_mut(), //Desired addr
            new_map.map_size, //size of mapping
            ProtFlags::PROT_READ|ProtFlags::PROT_WRITE, //Permissions on pages
            MapFlags::MAP_SHARED, //What kind of mapping
            new_map.map_fd, //fd
            0   //Offset into fd
        )
    } {
        Ok(v) => v as *mut c_void,
        Err(nix::Error::Sys(e)) => return Err(SharedMemError::MapCreateFailed(e as u32)),
        _ => return Err(SharedMemError::UnknownOsError(0xffff_ffff)),
    };

    Ok(new_map)
}

//Opens an existing mapping specified by its uid
pub fn open_mapping(unique_id: &str) -> Result<MapData, SharedMemError> {
    //Open shared memory
    let shmem_fd = match shm_open(
        unique_id,
        OFlag::O_RDWR, //Open read write
        Mode::S_IRUSR
    ) {
        Ok(v) => v,
        Err(nix::Error::Sys(e)) => return Err(SharedMemError::MapOpenFailed(e as u32)),
        _ => return Err(SharedMemError::UnknownOsError(0xffff_ffff)),
    };

    let mut new_map: MapData = MapData {
        owner: false,
        pid: 0,
        unique_id: String::from(unique_id),
        map_fd: shmem_fd,
        map_size: 0,
        map_ptr: null_mut(),
    };

    //Get mmap size
    new_map.map_size = match fstat(new_map.map_fd) {
        Ok(v) => v.st_size as usize,
        Err(nix::Error::Sys(e)) => return Err(SharedMemError::MapOpenFailed(e as u32)),
        _ => return Err(SharedMemError::UnknownOsError(0xffff_ffff)),
    };

    //Map memory into our address space
    new_map.map_ptr = match unsafe {
        mmap(null_mut(), //Desired addr
            new_map.map_size, //size of mapping
            ProtFlags::PROT_READ|ProtFlags::PROT_WRITE, //Permissions on pages
            MapFlags::MAP_SHARED, //What kind of mapping
            new_map.map_fd, //fd
            0   //Offset into fd
        )
    } {
        Ok(v) => v as *mut c_void,
        Err(nix::Error::Sys(e)) => return Err(SharedMemError::MapOpenFailed(e as u32)),
        _ => return Err(SharedMemError::UnknownOsError(0xffff_ffff)),
    };

    Ok(new_map)
}

//This functions exports our implementation for each lock type
pub fn lockimpl_from_type(lock_type: LockType) -> &'static dyn LockImpl {
    match lock_type {
        LockType::Mutex => &Mutex{},
        LockType::RwLock => &RwLock{},
    }
}

//This functions exports our implementation for each event type
pub fn eventimpl_from_type(event_type: EventType) -> &'static dyn EventImpl {
    match event_type {
        EventType::AutoBusy => &AutoBusy{},
        EventType::ManualBusy => &ManualBusy{},
        EventType::Manual => &ManualGeneric{},
        EventType::Auto => &AutoGeneric{},
        #[cfg(target_os="linux")]
        EventType::AutoEventFd => &AutoEventFd{},
        #[cfg(target_os="linux")]
        EventType::ManualEventFd => &ManualEventFd{},
    }
}

fn timeout_to_abstimespec(timeout: Timeout) -> timespec {
    let mut cur_time: timespec = timespec {
        tv_sec: -1,
        tv_nsec: 0,
    };
    match timeout {
        Timeout::Infinite => {},
        Timeout::Sec(t) => {
            unsafe {clock_gettime(CLOCK_REALTIME, &mut cur_time)};
            cur_time.tv_sec += t as time_t;
        },
        Timeout::Milli(t) => {
            unsafe {clock_gettime(CLOCK_REALTIME, &mut cur_time)};
            cur_time.tv_nsec += (t * 1_000_000) as c_long;
        },
        Timeout::Micro(t) => {
            unsafe {clock_gettime(CLOCK_REALTIME, &mut cur_time)};
            cur_time.tv_nsec += (t * 1_000) as c_long;
        },
        Timeout::Nano(t) => {
            unsafe {clock_gettime(CLOCK_REALTIME, &mut cur_time)};
            cur_time.tv_nsec += t as c_long;
        },
    };
    cur_time
}

/* Lock Implementations */

fn new_mutex(mutex: *mut pthread_mutex_t) -> Result<(), SharedMemError> {
    let mut res: libc::c_int;

    let mut lock_attr: pthread_mutexattr_t = unsafe {std::mem::zeroed()};

    //Set the PTHREAD_PROCESS_SHARED attribute on our rwlock
    res = unsafe{pthread_mutexattr_init(&mut lock_attr)};
    if res != 0 {
        return Err(SharedMemError::FailedToCreateLock(res as u32));
    }
    res = unsafe{pthread_mutexattr_setpshared(&mut lock_attr, PTHREAD_PROCESS_SHARED)};
    if res != 0 {
        return Err(SharedMemError::FailedToCreateLock(res as u32));
    }
    //Init the rwlock
    res = unsafe{pthread_mutex_init(mutex, &lock_attr)};
    if res != 0 {
        return Err(SharedMemError::FailedToCreateLock(res as u32));
    }
    Ok(())
}

fn mutex_lock(mutex: *mut pthread_mutex_t, abs_timeout_time: &timespec) -> Result<(), SharedMemError> {

    let res: libc::c_int;

    if abs_timeout_time.tv_sec == -1 {
        res = unsafe {pthread_mutex_lock(mutex)};
        if res != 0 {
            return Err(SharedMemError::FailedToAcquireLock(res as u32));
        }
        return Ok(())
    }

    res = unsafe{pthread_mutex_timedlock(mutex, abs_timeout_time)};

    if res == 0 {
        Ok(())
    } else if res == libc::ETIMEDOUT {
        Err(SharedMemError::Timeout)
    } else {
        Err(SharedMemError::FailedToAcquireLock(res as u32))
    }
}

fn mutex_unlock(mutex: *mut pthread_mutex_t) -> Result<(), SharedMemError> {

    let res: libc::c_int = unsafe {pthread_mutex_unlock(mutex)};

    if res != 0 {
        Err(SharedMemError::FailedToAcquireLock(res as u32))
    } else {
        Ok(())
    }
}

//Mutex
pub struct Mutex {}
impl LockImpl for Mutex {

    fn size_of(&self) -> usize {
        size_of::<pthread_mutex_t>()
    }
    fn init(&self, lock_info: &mut GenericLock, create_new: bool) -> Result<(), SharedMemError> {
        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        new_mutex(lock_info.lock_ptr as *mut pthread_mutex_t)
    }
    fn destroy(&self, _lock_info: &mut GenericLock) {}
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<(), SharedMemError> {
        mutex_lock(lock_ptr as *mut pthread_mutex_t, &timeout_to_abstimespec(Timeout::Infinite))
    }
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<(), SharedMemError> {
        mutex_lock(lock_ptr as *mut pthread_mutex_t, &timeout_to_abstimespec(Timeout::Infinite))
    }
    fn runlock(&self, lock_ptr: *mut c_void) {
        match mutex_unlock(lock_ptr as *mut pthread_mutex_t) {_=>{},};
    }
    fn wunlock(&self, lock_ptr: *mut c_void) {
        match mutex_unlock(lock_ptr as *mut pthread_mutex_t) {_=>{},};
    }
}

//RwLock
pub struct RwLock {}
impl LockImpl for RwLock {

    fn size_of(&self) -> usize {
        size_of::<pthread_rwlock_t>()
    }
    fn init(&self, lock_info: &mut GenericLock, create_new: bool) -> Result<(), SharedMemError> {
        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        let mut lock_attr: pthread_rwlockattr_t = unsafe{std::mem::zeroed()};
        unsafe {
          //Set the PTHREAD_PROCESS_SHARED attribute on our rwlock
          pthread_rwlockattr_init(&mut lock_attr);
          pthread_rwlockattr_setpshared(&mut lock_attr, PTHREAD_PROCESS_SHARED);
          //Init the rwlock
          pthread_rwlock_init(lock_info.lock_ptr as *mut pthread_rwlock_t, &lock_attr);
        }
        Ok(())
    }
    fn destroy(&self, _lock_info: &mut GenericLock) {}
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<(), SharedMemError> {
        unsafe {
            pthread_rwlock_rdlock(lock_ptr as *mut pthread_rwlock_t);
        }
        Ok(())
    }
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<(), SharedMemError> {
        unsafe {
            pthread_rwlock_wrlock(lock_ptr as *mut pthread_rwlock_t);
        }
        Ok(())
    }
    fn runlock(&self, lock_ptr: *mut c_void) {
        unsafe {
            pthread_rwlock_unlock(lock_ptr as *mut pthread_rwlock_t);
        }
    }
    fn wunlock(&self, lock_ptr: *mut c_void) {
        unsafe {
            pthread_rwlock_unlock(lock_ptr as *mut pthread_rwlock_t);
        }
    }
}

/* Event implementations */

fn new_eventcond(event: &mut EventCond) -> Result<(), SharedMemError> {
    /* Init signal state */
    event.signaled = false;
    let mut res: libc::c_int;

    /* Init the pthread_cond */
    let mut cond_attr: pthread_condattr_t = unsafe {std::mem::zeroed()};

    //Set the PTHREAD_PROCESS_SHARED attribute for our pthread_cond
    res = unsafe {pthread_condattr_init(&mut cond_attr)};
    if res != 0 {
        return Err(SharedMemError::FailedToCreateEvent(res as u32));
    }
    res = unsafe {pthread_condattr_setpshared(&mut cond_attr, PTHREAD_PROCESS_SHARED)};
    if res != 0 {
        return Err(SharedMemError::FailedToCreateEvent(res as u32));
    }
    //Init the pthread_cond
    res = unsafe {pthread_cond_init(&mut event.cond, &cond_attr)};
    if res != 0 {
        return Err(SharedMemError::FailedToCreateEvent(res as u32));
    }

    /* Init the pthread_mutex */
    new_mutex(&mut event.mutex)
}

fn event_wait(event: &mut EventCond, abs_timeout_time: &timespec, auto: bool) -> Result<(), SharedMemError> {
    let mut res: libc::c_int = 0;

    //Lock mutex for our pthread_cond
    mutex_lock(&mut (event.mutex), abs_timeout_time)?;

    while !event.signaled {
        //Timeout::Infinite
        if abs_timeout_time.tv_sec == -1 {
            res = unsafe{pthread_cond_wait(&mut event.cond, &mut event.mutex)};
        } else {
            res = unsafe{pthread_cond_timedwait(&mut (event.cond), &mut (event.mutex), abs_timeout_time)};
        }

        //Error hapenned
        if res != 0 {
            break;
        }
    }

    if res == 0 && auto {
        event.signaled = false;
    }

    match mutex_unlock(&mut event.mutex) {_=>{},};

    if res == libc::ETIMEDOUT {
        Err(SharedMemError::Timeout)
    } else if res != 0 {
        Err(SharedMemError::FailedToSignalEvent(res as u32))
    } else {
        Ok(())
    }
}

fn event_set(event: &mut EventCond, state: EventState, abs_timeout_time: &timespec, auto: bool) -> Result<(), SharedMemError> {

    mutex_lock(&mut event.mutex, abs_timeout_time)?;
    match state {
        EventState::Wait => event.signaled = false,
        EventState::Signaled => {
            event.signaled = true;
            let res = unsafe {
                if auto {
                    //Only unblock one thread as the signal will get reset anyways
                    pthread_cond_signal(&mut event.cond)
                } else {
                    //Unblock all threads, event will stay signaled
                    pthread_cond_broadcast(&mut event.cond)
                }
            };

            if res != 0 {
                mutex_unlock(&mut event.mutex)?;
                return Err(SharedMemError::FailedToSignalEvent(res as u32));
            }
        }
    };
    match mutex_unlock(&mut event.mutex) {_=>{},};

    Ok(())
}

struct EventCond {
    cond: pthread_cond_t,
    mutex: pthread_mutex_t,
    signaled: bool,
}
pub struct AutoGeneric {}
impl EventImpl for AutoGeneric {
    ///Returns the size of the event structure that will live in shared memory
    fn size_of(&self) -> usize {
        // + 3 allows us to move our EventCond to align it in the shmem
        size_of::<EventCond>()
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<(), SharedMemError> {

        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        let shared_event: &mut EventCond = unsafe {&mut (*(event_info.ptr as *mut EventCond))};

        new_eventcond(shared_event)
    }
    ///De-initializes the event
    fn destroy(&self, _event_info: &mut GenericEvent) {
        //Nothing to do here
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<(), SharedMemError> {
        let event: &mut EventCond = unsafe {&mut (*(event_ptr as *mut EventCond))};
        //Wait for the event, automatically reset signal state
        event_wait(event, &timeout_to_abstimespec(timeout), true)
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<(), SharedMemError> {
        let event: &mut EventCond = unsafe {&mut (*(event_ptr as *mut EventCond))};
        //Set event using pthread_cond_signal
        event_set(event, state, &timeout_to_abstimespec(Timeout::Infinite), true)
    }
}

pub struct ManualGeneric {}
impl EventImpl for ManualGeneric {
    ///Returns the size of the event structure that will live in shared memory
    fn size_of(&self) -> usize {
        // + 3 allows us to move our EventCond to align it in the shmem
        size_of::<EventCond>()
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<(), SharedMemError> {

        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        let shared_event: &mut EventCond = unsafe {&mut (*(event_info.ptr as *mut EventCond))};

        new_eventcond(shared_event)
    }
    ///De-initializes the event
    fn destroy(&self, _event_info: &mut GenericEvent) {
        //Nothing to do here
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<(), SharedMemError> {
        let event: &mut EventCond = unsafe {&mut (*(event_ptr as *mut EventCond))};
        //Wait for the event, dont reset signal state
        event_wait(event, &timeout_to_abstimespec(timeout), false)
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<(), SharedMemError> {
        let event: &mut EventCond = unsafe {&mut (*(event_ptr as *mut EventCond))};
        //Set event using pthread_cond_broadcast
        event_set(event, state, &timeout_to_abstimespec(Timeout::Infinite), false)
    }
}
