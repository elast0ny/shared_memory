extern crate nix;
extern crate libc;

use super::*;
use self::libc::{
    pthread_rwlock_t,
    pthread_rwlock_init,
    pthread_rwlock_unlock,
    /*
    pthread_rwlock_tryrdlock,
    pthread_rwlock_trywrlock,
    */
    pthread_rwlock_rdlock,
    pthread_rwlock_wrlock,
};

use self::nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, ProtFlags, MapFlags};
use self::nix::sys::stat::{fstat, FileStat, Mode};
use self::nix::fcntl::OFlag;
use self::nix::unistd::{close, ftruncate};

use std::os::raw::c_void;
use std::slice;
use std::os::unix::io::RawFd;
use std::ptr::{null_mut};
use std::mem::size_of;
use std::fs::{File};
use std::io::{Write, Read};

type Result<T> = std::result::Result<T, Box<std::error::Error>>;


impl<'a, T> MemFileRLockSlice<'a, T> {
    #[doc(hidden)] pub fn os_unlock(&mut self) {
        unsafe {pthread_rwlock_unlock(self.lock as *mut pthread_rwlock_t)};
    }
}
impl<'a, T> MemFileWLockSlice<'a, T> {
    #[doc(hidden)] pub fn os_unlock(&mut self) {
        unsafe {pthread_rwlock_unlock(self.lock as *mut pthread_rwlock_t)};
    }
}

///This struct lives insides the shared memory
struct MemCtl {
    ///Lock controlling the access to the mapping
    rw_lock: pthread_rwlock_t,
}

///This struct describes our memory mapping
pub struct MemMetadata {
    ///True if we created the mapping
    owner: bool,
    ///Name of mapping
    map_name: String,
    ///File descriptor from shm_open()
    map_fd: RawFd,
    ///Hold data to control the mapping (locks)
    map_ctl: *mut MemCtl,
    ///Holds the actual sizer of the mapping
    map_size: usize,
    ///Pointer to user data
    map_data: *mut c_void,
}

impl MemMetadata {
    ///Gets a reference to the shared memory as a slice of T with size elements
    ///This lock can be held by multiple readers
    ///Caller must validate the parameters
    pub fn read_lock_slice<T>(&self, start_offset: usize, num_elements:usize) -> MemFileRLockSlice<T> {
        unsafe {
            //Acquire read lock
            pthread_rwlock_rdlock(&mut (*self.map_ctl).rw_lock);
            MemFileRLockSlice {
                data: slice::from_raw_parts((self.map_data as usize + start_offset) as *const T, num_elements),
                lock: &mut (*self.map_ctl).rw_lock as *mut _ as *mut c_void,
            }
        }
    }

    ///Gets an exclusive mutable reference to the shared memory
    ///Caller must validate the parameters
    pub fn write_lock_slice<T>(&mut self, start_offset: usize, num_elements:usize) -> MemFileWLockSlice<T> {
        unsafe{
            //acquire write lock
            pthread_rwlock_wrlock(&mut (*self.map_ctl).rw_lock);
            MemFileWLockSlice {
                data: slice::from_raw_parts_mut((self.map_data as usize + start_offset) as *mut T, num_elements),
                lock: &mut (*self.map_ctl).rw_lock as *mut _ as *mut c_void,
            }
        }
    }
}

impl Drop for MemMetadata {
    ///Takes care of properly closing the MemFile (munmap(), shmem_unlink(), close())
    fn drop(&mut self) {

        //Unmap memory
        if !self.map_ctl.is_null() {
            match unsafe {munmap(self.map_ctl as *mut _, self.map_size)} {
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

impl MemFile {
    ///Opens an existing MemFile, shm_open()s it then mmap()s it
    pub fn os_open(mut new_file: MemFile) -> Result<MemFile> {

        let map_name: String;
        {
            //Get namespace of shared memory
            let mut disk_file = File::open(&new_file.file_path)?;
            let mut file_contents: Vec<u8> = Vec::with_capacity(new_file.file_path.to_string_lossy().len() + 5);
            disk_file.read_to_end(&mut file_contents)?;
            map_name = String::from_utf8(file_contents)?;
        }

        let mut meta: MemMetadata = MemMetadata {
            owner: new_file.owner,
            map_name: map_name,
            map_fd: 0,
            map_ctl: null_mut(),
            map_size: 0,
            map_data: null_mut(),
        };

        //Open shared memory
        meta.map_fd = match shm_open(
            meta.map_name.as_str(),
            OFlag::O_RDWR, //open for reading only
            Mode::S_IRUSR  //open for reading only
        ) {
            Ok(v) => v,
            Err(e) => return Err(From::from(format!("shm_open() failed with :\n{}", e))),
        };
        let file_stat: FileStat = match fstat(meta.map_fd) {
            Ok(v) => v,
            Err(e) => {
                return Err(From::from(e));
            }
        };

        let actual_size: usize = file_stat.st_size as usize;
        new_file.size = actual_size - size_of::<MemCtl>();

        //Map memory into our address space
        let map_addr: *mut c_void = match unsafe {
            mmap(null_mut(), //Desired addr
                actual_size, //size of mapping
                ProtFlags::PROT_READ|ProtFlags::PROT_WRITE, //Permissions on pages
                MapFlags::MAP_SHARED, //What kind of mapping
                meta.map_fd, //fd
                0   //Offset into fd
            )
        } {
            Ok(v) => v as *mut c_void,
            Err(e) => return Err(From::from(format!("mmap() failed with :\n{}", e))),
        };

        //Create control structures for the mapping
        meta.map_ctl = map_addr as *mut _;
        //Save the actual size of the mapping
        meta.map_size = actual_size;
        //Init pointer to user data
        meta.map_data = (map_addr as usize + size_of::<MemCtl>()) as *mut c_void;
        //This meta struct is now link to the MemFile
        new_file.meta = Some(meta);


        Ok(new_file)
    }

    ///Creates a new MemFile, shm_open()s it then mmap()s it
    pub fn os_create(mut new_file: MemFile) -> Result<MemFile> {

        let mut disk_file = File::create(&new_file.file_path)?;
        //println!("File created !");
        if !new_file.file_path.is_file() {
            return Err(From::from("Failed to create file"));
        }

        //Get unique name for mem mapping
        let abs_disk_path: PathBuf = new_file.file_path.canonicalize()?;
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

        let mut colision: usize = 0;

        loop {
            let shmem_path: PathBuf = match colision {
                0 => PathBuf::from(String::from("/dev/shm") + &unique_name),
                num => PathBuf::from(String::from("/dev/shm") + &unique_name + &format!("_{}", num)),
            };

            if !shmem_path.is_file() {
                if colision > 0 {
                    unique_name = String::from(unique_name + &format!("_{}", colision));
                }
                break;
            } else {
                println!("WARNING : File {} already exists. Did it not get properly closed ?", shmem_path.to_string_lossy());
                colision += 1;
            }
        }

        let mut meta: MemMetadata = MemMetadata {
            owner: new_file.owner,
            map_name: unique_name,
            map_fd: 0,
            map_ctl: null_mut(),
            map_size: 0,
            map_data: null_mut(),
        };

        //Create shared memory
        meta.map_fd = match shm_open(
            meta.map_name.as_str(), //Unique name that usualy pops up in /dev/shm/
            OFlag::O_CREAT|OFlag::O_EXCL|OFlag::O_RDWR, //create exclusively (error if collision) and read/write to allow resize
            Mode::S_IRUSR|Mode::S_IWUSR //Permission allow user+rw
        ) {
            Ok(v) => v,
            Err(e) => return Err(From::from(format!("shm_open() failed with :\n{}", e))),
        };

        //increase size to requested size + meta
        let actual_size: usize = new_file.size + size_of::<MemCtl>();

        match ftruncate(meta.map_fd, actual_size as i64) {
            Ok(_) => {},
            Err(e) => return Err(From::from(format!("ftruncate() failed with :\n{}", e))),
        };

        //Map memory into our address space
        let map_addr: *mut c_void = match unsafe {
            mmap(null_mut(), //Desired addr
                actual_size, //size of mapping
                ProtFlags::PROT_READ|ProtFlags::PROT_WRITE, //Permissions on pages
                MapFlags::MAP_SHARED, //What kind of mapping
                meta.map_fd, //fd
                0   //Offset into fd
            )
        } {
            Ok(v) => v as *mut c_void,
            Err(e) => return Err(From::from(format!("mmap() failed with :\n{}", e))),
        };

        //Initialise our metadata struct
        {
            //Create control structures for the mapping
            meta.map_ctl = map_addr as *mut _;
            //Save the actual size of the mapping
            meta.map_size = actual_size;
            //Init RwLock
            unsafe{
                pthread_rwlock_init(&mut (*meta.map_ctl).rw_lock, null_mut());
            }

            //Init pointer to user data
            meta.map_data = (map_addr as usize + size_of::<MemCtl>()) as *mut c_void;


            //println!("Created mapping of size 0x{:x} !", meta.map_size.unwrap());
            //println!("MetaHeader @ {:p}", meta.map_ctl.unwrap());
            //println!("Data @ {:p}", meta.map_data.unwrap());
        }

        //Write unique shmem name to disk
        match disk_file.write(&meta.map_name.as_bytes()) {
            Ok(write_sz) => if write_sz != meta.map_name.as_bytes().len() {
                return Err(From::from("Failed to write full contents info on disk"));
            },
            Err(_) => return Err(From::from("Failed to write info on disk")),
        };

        new_file.meta = Some(meta);

        Ok(new_file)
    }
}
