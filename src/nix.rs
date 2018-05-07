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
    LockNone,
    SharedMemLockImpl,
};

use std::os::raw::c_void;
use std::os::unix::io::RawFd;
use std::ptr::{null_mut};
use std::mem::size_of;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

#[cfg(target_os="macos")]
pub const MAX_NAME:usize = 30;
#[cfg(any(target_os="freebsd", target_os="linux"))]
pub const MAX_NAME:usize = 255;

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
        Err(nix::Error::Sys(Errno::EEXIST)) => return Err(From::from("RETRY")),
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

/*
//Opens an existing SharedMem, shm_open()s it then mmap()s it
pub fn open(mut new_file: SharedMem) -> Result<SharedMem> {


    //If there is a link file, this isnt a raw mapping
    let is_raw: bool = !new_file.link_path.is_some();

    // Get the shmem path
    let shmem_path = match new_file.real_path {
        Some(ref path) => path.clone(),
        None => {
            panic!("Tried to open SharedMem with no real_path");
        },
    };

    //Open shared memory
    let map_fd = mat
    ch shm_open(
        shmem_path.as_str(),
        OFlag::O_RDWR, //open for reading only
        Mode::S_IRUSR  //open for reading only
    ) {
        Ok(v) => v,
        Err(e) => return Err(From::from(format!("shm_open() failed with :\n{}", e))),
    };

    new_file.real_path = Some(shmem_path.clone());

    //Get mmap size
    let file_stat: FileStat = match fstat(map_fd) {
        Ok(v) => v,
        Err(e) => {
            return Err(From::from(e));
        }
    };

    //Map memory into our address space
    let map_addr: *mut c_void = match unsafe {
        mmap(null_mut(), //Desired addr
            file_stat.st_size as usize, //size of mapping
            ProtFlags::PROT_READ|ProtFlags::PROT_WRITE, //Permissions on pages
            MapFlags::MAP_SHARED, //What kind of mapping
            map_fd, //fd
            0   //Offset into fd
        )
    } {
        Ok(v) => v as *mut c_void,
        Err(e) => {
            match close(map_fd) {_=>{},};
            return Err(From::from(format!("mmap() failed with :\n{}", e)))
        },
    };

    //Return SharedMem with no meta data or locks
    if is_raw {
        new_file.size = file_stat.st_size as usize;
        new_file.meta = Some(
            MemMetadata {
                owner: false,
                map_name: shmem_path,
                map_fd: map_fd,
                map_size: new_file.size,
                shared_data: map_addr as *mut SharedData,
                lock_data: null_mut(),
                data: map_addr as *mut c_void,
                lock_impl: &LockNone {},
            }
        );

        return Ok(new_file);
    }

    //Figure out what the lock type is based on the shared_data set by create()
    let shared_data: &SharedData = unsafe {&(*(map_addr as *mut SharedData))};
    let lock_info = supported_locktype_from_ind(shared_data.lock_ind as usize);
    let lock_type: LockType = lock_info.0;

    //Ensure our shared data is 4 byte aligned
    let shared_data_sz = (size_of::<SharedData>() + 3) & !(0x03 as usize);
    let lock_data_sz = lock_info.1;

    let mut error: Option<String> = None;

    //Do some validation
    if shared_data.user_size == 0 {
        error = Some(String::from("Shared memory size is invalid"));
    } else if (shared_data_sz + lock_data_sz) >= file_stat.st_size as usize {
        error = Some(String::from("Shared memory size is too small to hold our metadata"));
    } else if shared_data.user_size > (file_stat.st_size as usize - (shared_data_sz + lock_data_sz)) {
        error = Some(String::from("Shared memory size does not match claimed size"));
    }

    if let Some(e) = error {
        match unsafe {munmap(map_addr as *mut _, file_stat.st_size as usize)} {_=>{},};
        match close(map_fd) {_=>{},};
        return Err(From::from(e));
    }

    new_file.size = shared_data.user_size;

    let meta: MemMetadata = MemMetadata {
        owner: false,
        map_name: shmem_path,
        map_fd: map_fd,
        map_size: file_stat.st_size as usize,
        shared_data: map_addr as *mut SharedData,
        lock_data: (map_addr as usize + shared_data_sz) as *mut _,
        data: (map_addr as usize + shared_data_sz + lock_data_sz) as *mut c_void,
        lock_impl: match lock_type {
            LockType::None => &LockNone{},
            LockType::Mutex =>  &Mutex{},
            LockType::RwLock => &RwLock{},
        },
    };

    //This meta struct is now link to the SharedMem
    new_file.meta = Some(meta);

    Ok(new_file)
}

nix::Error::Sys(Errno::EEXIST) => {
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
println!("Unique shared memory name already exists... Trying with \"{}\"", real_path);
continue

*/

//Returns the index and size of the lock_type
pub fn locktype_size(lock_type: &LockType) -> usize {
    match lock_type {
        &LockType::None => LockNone::size_of(),
        &LockType::Mutex => Mutex::size_of(),
        &LockType::RwLock => RwLock::size_of(),
    }
}
//Returns the implementation a specific lock
pub fn lockimpl_from_type(lock_type: &LockType) -> &'static SharedMemLockImpl {
    match lock_type {
        &LockType::None => &LockNone{},
        &LockType::Mutex => &Mutex{},
        &LockType::RwLock => &RwLock{},
    }
}

/* Lock Implementations */
//Mutex
pub struct Mutex {}
impl SharedMemLockImpl for Mutex {

    fn size_of() -> usize {
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
          pthread_mutex_init(lock_info.ptr as *mut pthread_mutex_t, lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t);
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
impl SharedMemLockImpl for RwLock {

    fn size_of() -> usize {
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
          pthread_rwlock_init(lock_info.ptr as *mut pthread_rwlock_t, lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t);
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
