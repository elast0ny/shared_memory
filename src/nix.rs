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
use self::nix::sys::stat::{fstat, FileStat, Mode};
use self::nix::fcntl::OFlag;
use self::nix::unistd::{close, ftruncate};

use super::{std,
    MemFile,
    LockType,
    LockNone,
    MemFileLockImpl,
};

use std::path::PathBuf;
use std::os::raw::c_void;
use std::os::unix::io::RawFd;
use std::ptr::{null_mut};
use std::mem::size_of;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

//This struct will live in the shared memory
struct SharedData {
    //This field is used to transmit the locking mechanism to an openner
    lock_ind: u8,
}

pub struct MemMetadata<'a> {

    /* Optionnal implementation fields */

    ///True if we created the mapping. Need to shm_unlink when we own the link
    owner: bool,
    ///Linux specific shared AsMut
    shared_data: *mut SharedData,
    ///Name of mapping
    map_name: String,
    ///File descriptor from shm_open()
    map_fd: RawFd,
    ///Holds the actual sizer of the mapping
    map_size: usize,

    /* Mandatory fields */
    ///the shared memory for our lock
    pub lock_data: *mut c_void,
    ///Pointer to user data
    pub data: *mut c_void,
    //Our custom lock implementation
    pub lock_impl : &'a MemFileLockImpl,

}

///shared memory teardown for linux
impl<'a> Drop for MemMetadata<'a> {
    ///Takes care of properly closing the MemFile (munmap(), shmem_unlink(), close())
    fn drop(&mut self) {

        //Unmap memory
        if !self.shared_data.is_null() {
            match unsafe {munmap(self.shared_data as *mut _, self.map_size)} {
                Ok(_) => {
                    //println!("munmap()");
                },
                Err(e) => {
                    println!("Failed to unmap memory while dropping MemFile !");
                    println!("{}", e);
                },
            };
        }

        //Unlink shmem
        if self.map_fd != 0 {
            //unlink shmem if we created it
            if self.owner {
                match shm_unlink(self.map_name.as_str()) {
                    Ok(_) => {
                        //println!("shm_unlink()");
                    },
                    Err(e) => {
                        println!("Failed to shm_unlink while dropping MemFile !");
                        println!("{}", e);
                    },
                };
            }

            match close(self.map_fd) {
                Ok(_) => {
                    //println!("close()");
                },
                Err(e) => {
                    println!("Failed to close shmem fd while dropping MemFile !");
                    println!("{}", e);
                },
            };
        }
    }
}

//Opens an existing MemFile, shm_open()s it then mmap()s it
pub fn open(mut new_file: MemFile) -> Result<MemFile> {


    //If there is a link file, this isnt a raw mapping
    let is_raw: bool = !new_file.link_path.is_some();

    // Get the shmem path
    let shmem_path = match new_file.real_path {
        Some(ref path) => path.clone(),
        None => {
            panic!("Tried to open MemFile with no real_path");
        },
    };

    println!("Openning shared mem \"{}\"", shmem_path);

    //Open shared memory
    let map_fd = match shm_open(
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

    //Return memfile with no meta data or locks
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
    //Set the proper user data size considering our metadata
    new_file.size = meta.map_size - shared_data_sz - lock_data_sz;

    //This meta struct is now link to the MemFile
    new_file.meta = Some(meta);

    Ok(new_file)
}

//Creates a new MemFile, shm_open()s it then mmap()s it
pub fn create(mut new_file: MemFile, lock_type: LockType) -> Result<MemFile> {

    // real_path is either :
    // 1. Specified directly
    // 2. Needs to be generated (link_file needs to exist)

    let is_raw = new_file.real_path.is_some();
    let real_path: String;
    //The user specified a real_path (raw mode)
    if is_raw {
        real_path = new_file.real_path.as_ref().unwrap().clone();
    //We will generate our our real_path
    } else {
        let link_path: &PathBuf = match new_file.link_path {
            Some(ref path) => path,
            None => panic!("Trying to create MemFile without link_path set"),
        };

        let abs_disk_path: PathBuf = link_path.canonicalize()?;
        let chars = abs_disk_path.to_string_lossy();
        let mut unique_name: String = String::with_capacity(chars.len());
        let mut chars = chars.chars();
        chars.next();
        unique_name.push('/');
        for c in chars {
            match c {
                '/' | '.' => unique_name.push('_'),
                v => unique_name.push(v),
            };
        }
        real_path = unique_name;
    }

    //Make sure we support this LockType
    let locktype_info = supported_locktype_info(&lock_type);

    let mut shared_data_sz: usize = 0;
    let mut lock_ind: u8 = 0;
    let mut lock_data_sz: usize = 0;

    //Set our meta data sizes if this is not a raw memfile
    if !is_raw {
        shared_data_sz = (size_of::<SharedData>() + 3) & !(0x03 as usize);
        lock_ind = locktype_info.0 as u8;
        lock_data_sz = locktype_info.1;
    }

    //Create shared memory
    let shmem_fd = match shm_open(
        real_path.as_str(), //Unique name that usualy pops up in /dev/shm/
        OFlag::O_CREAT|OFlag::O_EXCL|OFlag::O_RDWR, //create exclusively (error if collision) and read/write to allow resize
        Mode::S_IRUSR|Mode::S_IWUSR //Permission allow user+rw
    ) {
        Ok(v) => v,
        Err(e) => return Err(From::from(format!("shm_open() failed with :\n{}", e))),
    };
    new_file.real_path = Some(real_path.clone());

    //increase size to requested size + meta
    let actual_size: usize = new_file.size + lock_data_sz + shared_data_sz;

    #[cfg(target_arch="x86")]
    let size: i32 = actual_size as i32;
    #[cfg(target_arch="x86_64")]
    let size: i64 = actual_size as i64;

    match ftruncate(shmem_fd, size) {
        Ok(_) => {},
        Err(e) => {
            match shm_unlink(real_path.as_str()) {_=>{},};
            match close(shmem_fd) {_=>{},};
            return Err(From::from(format!("ftruncate() failed with :\n{}", e)))
        },
    };

    //Map memory into our address space
    let map_addr: *mut c_void = match unsafe {
        mmap(null_mut(), //Desired addr
            actual_size, //size of mapping
            ProtFlags::PROT_READ|ProtFlags::PROT_WRITE, //Permissions on pages
            MapFlags::MAP_SHARED, //What kind of mapping
            shmem_fd, //fd
            0   //Offset into fd
        )
    } {
        Ok(v) => v as *mut c_void,
        Err(e) => {
            match shm_unlink(real_path.as_str()) {_=>{},};
            match close(shmem_fd) {_=>{},};
            return Err(From::from(format!("mmap() failed with :\n{}", e)))
        },
    };


    //Nothing else to do if raw mapping
    if is_raw {
        new_file.meta = Some(MemMetadata {
            owner: true,
            map_name: real_path,
            map_fd: shmem_fd,
            map_size: actual_size,
            shared_data: map_addr as *mut SharedData,
            lock_data: null_mut(),
            data: map_addr as *mut c_void,
            lock_impl: &LockNone{},
        });

        return Ok(new_file);
    }

    let mut meta = MemMetadata {
        owner: true,
        map_name: real_path,
        map_fd: shmem_fd,
        map_size: actual_size,
        shared_data: map_addr as *mut SharedData,
        lock_data: (map_addr as usize + shared_data_sz) as *mut _,
        data: (map_addr as usize + shared_data_sz + lock_data_sz) as *mut c_void,
        lock_impl: &LockNone{},
    };

    //Init our shared metadata
    let shared_data: &mut SharedData = unsafe {
        &mut (*meta.shared_data)
    };
    shared_data.lock_ind = lock_ind;

    //Init Lock data
    match lock_type {
        LockType::None => {},
        LockType::Mutex => {
            let mut lock_attr: [u8; size_of::<pthread_mutexattr_t>()] = [0; size_of::<pthread_mutexattr_t>()];
            unsafe {
                //Set the PTHREAD_PROCESS_SHARED attribute on our rwlock
                pthread_mutexattr_init(lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t);
                pthread_mutexattr_setpshared(lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t, PTHREAD_PROCESS_SHARED);
                //Init the rwlock
                pthread_mutex_init(meta.lock_data as *mut pthread_mutex_t, lock_attr.as_mut_ptr() as *mut pthread_mutexattr_t);
            }
            meta.lock_impl = &Mutex{};
        },
        LockType::RwLock => {
            // Init our RW lock
            let mut lock_attr: [u8; size_of::<pthread_rwlockattr_t>()] = [0; size_of::<pthread_rwlockattr_t>()];
            unsafe {
                //Set the PTHREAD_PROCESS_SHARED attribute on our rwlock
                pthread_rwlockattr_init(lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t);
                pthread_rwlockattr_setpshared(lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t, PTHREAD_PROCESS_SHARED);
                //Init the rwlock
                pthread_rwlock_init(meta.lock_data as *mut pthread_rwlock_t, lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t);
            }
            meta.lock_impl = &RwLock{};
        },
    };

    new_file.meta = Some(meta);
    Ok(new_file)
}

//Returns the index and size of the lock_type
fn supported_locktype_info(lock_type: &LockType) -> (usize, usize) {
    match lock_type {
        &LockType::None => (0, LockNone::size_of()),
        &LockType::Mutex => (1, Mutex::size_of()),
        &LockType::RwLock => (2, RwLock::size_of()),
    }
}

//Returns the proper locktype and size of the structure
fn supported_locktype_from_ind(index: usize) -> (LockType, usize) {
    match index {
        0 => (LockType::None, LockNone::size_of()),
        1 => (LockType::Mutex, Mutex::size_of()),
        2 => (LockType::RwLock, RwLock::size_of()),
        _ => unimplemented!("Linux does not support this locktype index..."),
    }
}

/* Lock Implementations */
//Mutex
pub struct Mutex {}
impl MemFileLockImpl for Mutex {

    fn size_of() -> usize {
        size_of::<pthread_mutex_t>()
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
impl MemFileLockImpl for RwLock {

    fn size_of() -> usize {
        size_of::<pthread_rwlock_t>()
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
