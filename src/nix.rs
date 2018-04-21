extern crate libc;
extern crate nix;

use self::libc::{
    pthread_rwlock_t,
    pthread_rwlock_init,
    pthread_rwlock_unlock,
    pthread_rwlock_rdlock,
    pthread_rwlock_wrlock,

    //pthread_rwlock_tryrdlock,
    //pthread_rwlock_trywrlock,
    /* Lock Attribute stuff */
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
}

pub struct MemMetadata<'a> {

    /* Optionnal implementation fields */

    ///True if we created the mapping
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

    // Get the shmem path
    let shmem_path = match new_file.real_path {
        Some(ref path) => path.clone(),
        None => {
            panic!("Tried to open MemFile with no real_path");
        },
    };

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

    //Figure out what the lock type is based on the shared_data set by create()
    let lock_ind = unsafe {(*(map_addr as *mut SharedData)).lock_ind};
    let lock_type: LockType = ind_to_locktype(&(lock_ind as usize));

    //Ensure our shared data is 4 byte aligned
    let shared_data_sz = (size_of::<SharedData>() + 3) & !(0x03 as usize);
    let lock_data_sz = get_supported_lock_size(&lock_type);

    let meta: MemMetadata = MemMetadata {
        owner: false,
        map_name: shmem_path,
        map_fd: map_fd,
        map_size: file_stat.st_size as usize,
        shared_data: map_addr as *mut SharedData,
        lock_data: (map_addr as usize + shared_data_sz) as *mut _,
        data: (map_addr as usize + shared_data_sz + lock_data_sz) as *mut c_void,
        lock_impl: get_supported_lock(&lock_type),//get_supported_lock(&lock_type),
    };

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
                unique_name.push('/');
                for c in chars {
                    match c {
                        '/' | '.' => unique_name.push('_'),
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

    //Create shared memory
    //TODO : Handle "File exists" errors when creating MemFile with new_file.link_path.is_some()
    //       When new_file.link_path.is_some(), we can figure out a real_path that doesnt collide with another
    //       and stick it in the link_file
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

    match ftruncate(shmem_fd, actual_size as i64) {
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

    let meta = MemMetadata {
        owner: true,
        map_name: real_path,
        map_fd: shmem_fd,
        map_size: actual_size,
        shared_data: map_addr as *mut SharedData,
        lock_data: (map_addr as usize + shared_data_sz) as *mut _,
        data: (map_addr as usize + shared_data_sz + lock_data_sz) as *mut c_void,
        lock_impl: get_supported_lock(&lock_type),
    };

    //Write the type of lock we created the mapping with
    unsafe {
        (*meta.shared_data).lock_ind = locktype_to_ind(&lock_type) as u8;
    }

    //Link the finalized metadata to the MemFile
    new_file.meta = Some(meta);

    Ok(new_file)
}

//Returns the size we need to allocate in the shared memory for our lock
fn get_supported_lock_size(lock_type: &LockType) -> usize {
    match lock_type {
        &LockType::None => LockNone::size_of(),
        &LockType::Rwlock => RwLock::size_of(),
        _ => unimplemented!("Linux does not support this lock type..."),
    }
}

//Returns a boxed trait that implements MemFileLockImpl for the specified type
fn get_supported_lock(lock_type: &LockType) -> &'static MemFileLockImpl {
    match lock_type {
        &LockType::None => &LockNone{},
        &LockType::Rwlock => &RwLock{},
        _ => unimplemented!("Linux does not support this lock type..."),
    }
}

/* Lock Implementations */

pub struct RwLock {}

impl MemFileLockImpl for RwLock {
    //Init the rwlock with proper attributes
    fn init(&self, lock_ptr: *mut c_void) -> Result<()> {

        let mut lock_attr: [u8; size_of::<pthread_rwlockattr_t>()] = [0; size_of::<pthread_rwlockattr_t>()];
        unsafe {
            //Set the PTHREAD_PROCESS_SHARED attribute on our rwlock
            pthread_rwlockattr_init(lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t);
            pthread_rwlockattr_setpshared(lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t, PTHREAD_PROCESS_SHARED);
            //Init the rwlock
            pthread_rwlock_init(lock_ptr as *mut pthread_rwlock_t, lock_attr.as_mut_ptr() as *mut pthread_rwlockattr_t);
        }
        Ok(())
    }
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
