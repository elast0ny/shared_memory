extern crate libc;
extern crate nix;

cfg_if!{
    if #[cfg(target_os="linux")] {
        pub mod linux;
        pub use self::linux::*;
    }
}

use self::libc::{
    timespec,
    clock_gettime,
    CLOCK_REALTIME,

    //Mutex defs
    pthread_mutex_t,
    pthread_mutex_init,
    pthread_mutex_timedlock,
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

use self::nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, ProtFlags, MapFlags};
use self::nix::errno::Errno;
use self::nix::sys::stat::{fstat, Mode};
use self::nix::fcntl::OFlag;
use self::nix::unistd::{close, ftruncate};

use super::{std,
    AtomicBool,
    Ordering,
    LockType,
    GenericLock,
    LockImpl,
    EventType,
    EventImpl,
    EventState,
    Timeout,
    GenericEvent,
};

use std::os::raw::c_void;
use std::os::unix::io::RawFd;
use std::ptr::{null_mut};
use std::mem::size_of;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

/*
#[cfg(target_os="macos")]
pub const MAX_NAME:usize = 30;
#[cfg(any(target_os="freebsd", target_os="linux"))]
pub const MAX_NAME:usize = 255;
*/

pub struct MapData {

    //On linux, you must shm_unlink() the object created for the mapping. It wont disappear automatically.
    owner: bool,

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
                Err(e) => {
                    println!("os_impl::Linux : Failed to munmap() shared memory mapping...");
                    println!("{}", e);
                },
            };
        }

        //Unlink shmem
        if self.map_fd != 0 {
            //unlink shmem if we created it
            if self.owner {
                match shm_unlink(self.unique_id.as_str()) {
                    Ok(_) => {
                        //println!("shm_unlink()");
                    },
                    Err(e) => {
                        println!("os_impl::Linux : Failed to shm_unlink() shared memory name...");
                        println!("{}", e);
                    },
                };
            }

            match close(self.map_fd) {
                Ok(_) => {
                    //println!("close()");
                },
                Err(e) => {
                    println!("os_impl::Linux : Failed to close() shared memory file descriptor...");
                    println!("{}", e);
                },
            };
        }
    }
}

//Creates a mapping specified by the uid and size
pub fn create_mapping(unique_id: &str, map_size: usize) -> Result<MapData> {

    //Create shared memory file descriptor
    let shmem_fd = match shm_open(
        unique_id, //Unique name that usualy pops up in /dev/shm/
        OFlag::O_CREAT|OFlag::O_EXCL|OFlag::O_RDWR, //create exclusively (error if collision) and read/write to allow resize
        Mode::S_IRUSR|Mode::S_IWUSR //Permission allow user+rw
    ) {
        Ok(v) => v,
        Err(nix::Error::Sys(Errno::EEXIST)) => return Err(From::from("NAME_EXISTS")),
        e => return Err(From::from(format!("shm_open() failed with :\n{:?}", e))),
    };

    let mut new_map: MapData = MapData {
        owner: true,
        unique_id: String::from(unique_id),
        map_fd: shmem_fd,
        map_size: map_size,
        map_ptr: null_mut(),
    };

    //Enlarge the memory descriptor file size to the requested map size
    match ftruncate(new_map.map_fd, new_map.map_size as _) {
        Ok(_) => {},
        Err(e) => return Err(From::from(format!("ftruncate() failed with :\n{}", e))),
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
        Err(e) => return Err(From::from(format!("mmap() failed with :\n{}", e))),
    };

    Ok(new_map)
}

//Opens an existing mapping specified by its uid
pub fn open_mapping(unique_id: &str) -> Result<MapData> {
    //Open shared memory
    let shmem_fd = match shm_open(
        unique_id,
        OFlag::O_RDWR, //Open read write
        Mode::S_IRUSR
    ) {
        Ok(v) => v,
        Err(e) => return Err(From::from(format!("shm_open() failed with :\n{}", e))),
    };

    let mut new_map: MapData = MapData {
        owner: false,
        unique_id: String::from(unique_id),
        map_fd: shmem_fd,
        map_size: 0,
        map_ptr: null_mut(),
    };

    //Get mmap size
    new_map.map_size = match fstat(new_map.map_fd) {
        Ok(v) => v.st_size as usize,
        Err(e) => {
            return Err(From::from(e));
        }
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
        Err(e) => return Err(From::from(format!("mmap() failed with :\n{}", e))),
    };

    Ok(new_map)
}

//This functions exports our implementation for each lock type
pub fn lockimpl_from_type(lock_type: &LockType) -> &'static LockImpl {
    match lock_type {
        &LockType::Mutex => &Mutex{},
        &LockType::RwLock => &RwLock{},
    }
}

//This functions exports our implementation for each event type
pub fn eventimpl_from_type(event_type: &EventType) -> &'static EventImpl {
    match event_type {
        &EventType::AutoBusy => &AutoBusy{},
        &EventType::ManualBusy => &ManualBusy{},
        &EventType::Manual => &ManualGeneric{},
        &EventType::Auto => &AutoGeneric{},
        #[cfg(target_os="linux")]
        &EventType::AutoEventFd => &AutoEventFd{},
        #[cfg(target_os="linux")]
        &EventType::ManualEventFd => &ManualEventFd{},
    }
}

fn timeout_to_timespec(timeout: Timeout) -> timespec {
    let mut cur_time: timespec = timespec {
        tv_sec: -1,
        tv_nsec: 0,
    };
    match timeout {
        Timeout::Infinite => {},
        Timeout::Sec(t) => {
            unsafe {clock_gettime(CLOCK_REALTIME, &mut cur_time)};
            cur_time.tv_sec += t as i64;
        },
        Timeout::Milli(t) => {
            unsafe {clock_gettime(CLOCK_REALTIME, &mut cur_time)};
            cur_time.tv_nsec += (t * 1_000_000) as i64;
        },
        Timeout::Micro(t) => {
            unsafe {clock_gettime(CLOCK_REALTIME, &mut cur_time)};
            cur_time.tv_nsec += (t * 1_000) as i64;
        },
        Timeout::Nano(t) => {
            unsafe {clock_gettime(CLOCK_REALTIME, &mut cur_time)};
            cur_time.tv_nsec += t as i64;
        },
    };
    cur_time
}

fn timeout_to_duration(timeout: Timeout) -> Duration {
    Duration::from_millis(
        match timeout {
            Timeout::Infinite => !(0 as u64),
            Timeout::Sec(t) => (t * 1000) as u64,
            Timeout::Milli(t) => (t) as u64,
            Timeout::Micro(t) => (t / 1000) as u64,
            Timeout::Nano(t) => (t / 1000000) as u64,
        }
    )
}

/* Lock Implementations */

fn new_mutex(mutex: *mut pthread_mutex_t) -> Result<()> {
    let mut res: libc::c_int;

    let mut lock_attr: [u8; size_of::<pthread_mutexattr_t>()] = [0; size_of::<pthread_mutexattr_t>()];

    //Set the PTHREAD_PROCESS_SHARED attribute on our rwlock
    res = unsafe{pthread_mutexattr_init(lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t)};
    if res != 0 {
        return Err(From::from(format!("pthread_condattr_init() : Failed with {}", res)));
    }
    res = unsafe{pthread_mutexattr_setpshared(lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t, PTHREAD_PROCESS_SHARED)};
    if res != 0 {
        return Err(From::from(format!("pthread_condattr_init() : Failed with {}", res)));
    }
    //Init the rwlock
    res = unsafe{pthread_mutex_init(mutex, lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t)};
    if res != 0 {
        return Err(From::from(format!("pthread_condattr_init() : Failed with {}", res)));
    }
    Ok(())
}

fn mutex_lock(mutex: *mut pthread_mutex_t, abs_timeout_time: &timespec) -> Result<()> {

    let res: libc::c_int;

    if abs_timeout_time.tv_sec == -1 {
        res = unsafe {pthread_mutex_lock(mutex)};
        if res != 0 {
            return Err(From::from("pthread_mutex_lock() : Error acquiring mutex"));
        }
        return Ok(())
    }

    res = unsafe{pthread_mutex_timedlock(mutex, abs_timeout_time)};

    if res == 0 {
        Ok(())
    } else if res == libc::ETIMEDOUT {
        Err(From::from("pthread_mutex_timedlock() : Timed out"))
    } else {
        Err(From::from(format!("pthread_mutex_timedlock() : Error {}", res)))
    }
}

fn mutex_unlock(mutex: *mut pthread_mutex_t) -> Result<()> {

    let res: libc::c_int = unsafe {pthread_mutex_unlock(mutex)};

    if res != 0 {
        Err(From::from(format!("mutex_unlock() : Error {}", res)))
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
    fn init(&self, lock_info: &mut GenericLock, create_new: bool) -> Result<()> {
        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        new_mutex(lock_info.lock_ptr as *mut pthread_mutex_t)
    }
    fn destroy(&self, _lock_info: &mut GenericLock) {}
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        mutex_lock(lock_ptr as *mut pthread_mutex_t, &timeout_to_timespec(Timeout::Infinite))
    }
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        mutex_lock(lock_ptr as *mut pthread_mutex_t, &timeout_to_timespec(Timeout::Infinite))
    }
    fn runlock(&self, lock_ptr: *mut c_void) -> () {
        match mutex_unlock(lock_ptr as *mut pthread_mutex_t) {_=>{},};
    }
    fn wunlock(&self, lock_ptr: *mut c_void) -> () {
        match mutex_unlock(lock_ptr as *mut pthread_mutex_t) {_=>{},};
    }
}

//RwLock
pub struct RwLock {}
impl LockImpl for RwLock {

    fn size_of(&self) -> usize {
        size_of::<pthread_rwlock_t>()
    }
    fn init(&self, lock_info: &mut GenericLock, create_new: bool) -> Result<()> {
        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        let mut lock_attr: [u8; size_of::<pthread_rwlockattr_t>()] = [0; size_of::<pthread_rwlockattr_t>()];
        unsafe {
          //Set the PTHREAD_PROCESS_SHARED attribute on our rwlock
          pthread_rwlockattr_init(lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t);
          pthread_rwlockattr_setpshared(lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t, PTHREAD_PROCESS_SHARED);
          //Init the rwlock
          pthread_rwlock_init(lock_info.lock_ptr as *mut pthread_rwlock_t, lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t);
        }
        Ok(())
    }
    fn destroy(&self, _lock_info: &mut GenericLock) {}
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        unsafe {
            pthread_rwlock_rdlock(lock_ptr as *mut pthread_rwlock_t);
        }
        Ok(())
    }
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        unsafe {
            pthread_rwlock_wrlock(lock_ptr as *mut pthread_rwlock_t);
        }
        Ok(())
    }
    fn runlock(&self, lock_ptr: *mut c_void) -> () {
        unsafe {
            pthread_rwlock_unlock(lock_ptr as *mut pthread_rwlock_t);
        }
    }
    fn wunlock(&self, lock_ptr: *mut c_void) -> () {
        unsafe {
            pthread_rwlock_unlock(lock_ptr as *mut pthread_rwlock_t);
        }
    }
}

/* Event implementations */

fn new_eventcond(event: &mut EventCond) -> Result<()> {
    /* Init signal state */
    event.signaled = false;
    let mut res: libc::c_int;

    /* Init the pthread_cond */
    let mut cond_attr: [u8; size_of::<pthread_condattr_t>()] = [0; size_of::<pthread_condattr_t>()];

    //Set the PTHREAD_PROCESS_SHARED attribute for our pthread_cond
    res = unsafe {pthread_condattr_init(cond_attr.as_mut_ptr() as *mut pthread_condattr_t)};
    if res != 0 {
        return Err(From::from(format!("pthread_condattr_init() : Failed with {}", res)));
    }
    res = unsafe {pthread_condattr_setpshared(cond_attr.as_mut_ptr() as *mut pthread_condattr_t, PTHREAD_PROCESS_SHARED)};
    if res != 0 {
        return Err(From::from(format!("pthread_condattr_setpshared() : Failed with {}", res)));
    }
    //Init the pthread_cond
    res = unsafe {pthread_cond_init(&mut event.cond, cond_attr.as_mut_ptr() as *mut pthread_condattr_t)};
    if res != 0 {
        return Err(From::from(format!("pthread_cond_init() : Failed with {}", res)));
    }

    /* Init the pthread_mutex */
    new_mutex(&mut event.mutex)
}

fn event_wait(event: &mut EventCond, abs_timeout_time: &timespec, auto: bool) -> Result<()> {
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
        Err(From::from("pthread_cond_timedwait() : Time out"))
    } else if res != 0 {
        Err(From::from(format!("pthread_cond_[timed]wait() : Error {}", res)))
    } else {
        Ok(())
    }
}

fn event_set(event: &mut EventCond, state: EventState, abs_timeout_time: &timespec, auto: bool) -> Result<()> {

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
                return Err(From::from(format!("pthread_cond_[signal|broadcast]() : Failed with err {}", res)));
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
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<()> {

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
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<()> {
        let event: &mut EventCond = unsafe {&mut (*(event_ptr as *mut EventCond))};
        //Wait for the event, automatically reset signal state
        event_wait(event, &timeout_to_timespec(timeout), true)
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<()> {
        let event: &mut EventCond = unsafe {&mut (*(event_ptr as *mut EventCond))};
        //Set event using pthread_cond_signal
        event_set(event, state, &timeout_to_timespec(Timeout::Infinite), true)
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
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<()> {

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
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<()> {
        let event: &mut EventCond = unsafe {&mut (*(event_ptr as *mut EventCond))};
        //Wait for the event, dont reset signal state
        event_wait(event, &timeout_to_timespec(timeout), false)
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<()> {
        let event: &mut EventCond = unsafe {&mut (*(event_ptr as *mut EventCond))};
        //Set event using pthread_cond_broadcast
        event_set(event, state, &timeout_to_timespec(Timeout::Infinite), false)
    }
}

pub struct AutoBusy {}
impl EventImpl for AutoBusy {
    fn size_of(&self) -> usize {
        size_of::<AtomicBool>()
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<()> {

        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        let signal: &AtomicBool = unsafe {&mut (*(event_info.ptr as *mut AtomicBool))};
        signal.store(false, Ordering::Relaxed);

        Ok(())
    }
    ///De-initializes the event
    fn destroy(&self, _event_info: &mut GenericEvent) {
        //Nothing to do here
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<()> {

        let signal: &AtomicBool = unsafe {&mut (*(event_ptr as *mut AtomicBool))};

        let timeout_len: Duration = match timeout {
            Timeout::Infinite => {
                while !signal.compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed).is_ok() {}
                return Ok(())
            },
            _ => timeout_to_duration(timeout),
        };

        //let check_interval = 5;
        //let mut num_attemps: usize = 0;
        let start_time: Instant = Instant::now();

        //Busy loop checking timeout every 5 iterations
        while !signal.compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            //num_attemps = num_attemps.wrapping_add(1);
            //if num_attemps%check_interval == 0 {
            if start_time.elapsed() >= timeout_len {
                return Err(From::from("Timed out"));
            }
            //}
        }

        Ok(())
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<()> {
        let signal: &AtomicBool = unsafe {&mut (*(event_ptr as *mut AtomicBool))};

        signal.store(
            match state {
                EventState::Wait => false,
                EventState::Signaled => true,
            },
            Ordering::Relaxed
        );

        Ok(())
    }
}

pub struct ManualBusy {}
impl EventImpl for ManualBusy {
    fn size_of(&self) -> usize {
        size_of::<AtomicBool>()
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<()> {

        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        let signal: &AtomicBool = unsafe {&mut (*(event_info.ptr as *mut AtomicBool))};
        signal.store(false, Ordering::Relaxed);

        Ok(())
    }
    ///De-initializes the event
    fn destroy(&self, _event_info: &mut GenericEvent) {
        //Nothing to do here
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<()> {

        let signal: &AtomicBool = unsafe {&mut (*(event_ptr as *mut AtomicBool))};

        let timeout_len: Duration = match timeout {
            Timeout::Infinite => {
                while !signal.load(Ordering::Relaxed) {}
                return Ok(())
            },
            _ => timeout_to_duration(timeout),
        };

        //let check_interval = 5;
        //let mut num_attemps: usize = 0;
        let start_time: Instant = Instant::now();

        //Busy loop checking timeout every 5 iterations
        while !signal.load(Ordering::Relaxed) {
            //num_attemps = num_attemps.wrapping_add(1);
            //if num_attemps%check_interval == 0 {
            if start_time.elapsed() >= timeout_len {
                return Err(From::from("Timed out"));
            }
            //}
        }
        Ok(())
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<()> {
        let signal: &AtomicBool = unsafe {&mut (*(event_ptr as *mut AtomicBool))};

        signal.store(
            match state {
                EventState::Wait => false,
                EventState::Signaled => true,
            },
            Ordering::Relaxed
        );

        Ok(())
    }
}
