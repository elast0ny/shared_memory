extern crate libc;
extern crate nix;

use self::libc::{
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

    PTHREAD_PROCESS_SHARED,
};

use self::nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, ProtFlags, MapFlags};
use self::nix::errno::Errno;
use self::nix::sys::stat::{fstat, Mode};
use self::nix::fcntl::OFlag;
use self::nix::unistd::{close, ftruncate};

use super::{std,
    LockType,
    GenericLock,
    LockImpl,
};

use std::os::raw::c_void;
use std::os::unix::io::RawFd;
use std::ptr::{null_mut};
use std::mem::size_of;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

#[cfg(target_os="macos")]
pub const _MAX_NAME:usize = 30;
#[cfg(any(target_os="freebsd", target_os="linux"))]
pub const _MAX_NAME:usize = 255;

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

/* Lock Implementations */
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

        let mut lock_attr: [u8; size_of::<pthread_mutexattr_t>()] = [0; size_of::<pthread_mutexattr_t>()];
        unsafe {
          //Set the PTHREAD_PROCESS_SHARED attribute on our rwlock
          pthread_mutexattr_init(lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t);
          pthread_mutexattr_setpshared(lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t, PTHREAD_PROCESS_SHARED);
          //Init the rwlock
          pthread_mutex_init(lock_info.lock_ptr as *mut pthread_mutex_t, lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t);
        }
        Ok(())
    }
    fn rlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        unsafe {
            pthread_mutex_lock(lock_ptr as *mut pthread_mutex_t);
        }
        Ok(())
    }
    fn wlock(&self, lock_ptr: *mut c_void) -> Result<()> {
        unsafe {
            pthread_mutex_lock(lock_ptr as *mut pthread_mutex_t);
        }
        Ok(())
    }
    fn runlock(&self, lock_ptr: *mut c_void) -> () {
        unsafe {
            pthread_mutex_unlock(lock_ptr as *mut pthread_mutex_t);
        }
    }
    fn wunlock(&self, lock_ptr: *mut c_void) -> () {
        unsafe {
            pthread_mutex_unlock(lock_ptr as *mut pthread_mutex_t);
        }
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
