extern crate nix;
extern crate libc;

use super::*;
use self::libc::{c_void,
    pthread_rwlock_t,
    pthread_rwlock_init};

use self::nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, ProtFlags, MapFlags};
use self::nix::sys::stat::{fstat, FileStat, Mode};
use self::nix::fcntl::OFlag;
use self::nix::unistd::{close, ftruncate};

use std::slice;
use std::os::unix::io::RawFd;
use std::ptr::{null_mut};
use std::mem::size_of;
use std::fs::{File};
use std::io::{Write, Read};

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

struct MemCtl {
    rw_lock: pthread_rwlock_t,
}

pub struct MemMetadata {
    owner: bool,
    map_name: Option<String>,
    map_fd: Option<RawFd>,
    ///Hold data to control the mapping (locks)
    map_ctl: Option<*mut MemCtl>,
    ///Holds the actual sizer of the mapping
    map_size: Option<usize>,
    ///Pointer to user data
    map_data: Option<*mut c_void>,
}

impl MemMetadata {

    ///Wrapper to easily grab the rwlock of our mapping
    pub fn get_rwlock(&self) -> Option<&mut pthread_rwlock_t> {
        if let Some(map_ctl) = self.map_ctl {
            return Some(unsafe {&mut (*map_ctl).rw_lock});
        }
        None
    }

    pub fn get_mut_nolock(&self, size: usize) -> Option<&mut [u8]> {
        if let Some(data_addr) = self.map_data {
            Some( unsafe {slice::from_raw_parts_mut(data_addr as *mut u8, size)})
        } else {
            None
        }
    }
}

impl Drop for MemMetadata {
    fn drop(&mut self) {

        /*
        //Dont need to do, this invalidates the lock for other process that still might have it mapped
        if let Some(lock_addr) = self.get_rwlock() {
            unsafe {pthread_rwlock_destroy(lock_addr)};
        }
        */

        //Unmap memory
        if let Some(map_addr) = self.map_ctl {
            match unsafe {munmap(map_addr as *mut c_void, self.map_size.unwrap())} {
                Ok(_) => {println!("munmap()")},
                Err(e) => {
                    println!("Failed to unmap memory while dropping MemFile !");
                    println!("{}", e);
                },
            };
        }

        //Unlink shmem
        if let Some(fd) = self.map_fd {
            //unlink shmem if we created it
            if self.owner {
                match shm_unlink(self.map_name.as_ref().unwrap().as_str()) {
                    Ok(_) => {println!("shm_unlink()")},
                    Err(e) => {
                        println!("Failed to shm_unlink while dropping MemFile !");
                        println!("{}", e);
                    },
                };
            }

            match close(fd) {
                Ok(_) => {println!("close()")},
                Err(e) => {
                    println!("Failed to close shmem fd while dropping MemFile !");
                    println!("{}", e);
                },
            };
        }
    }
}

impl MemFile {
    pub fn os_open(mut new_file: MemFile) -> Result<MemFile> {

        let map_name: String;
        {
            //Get namespace of shared memory
            let mut disk_file = File::open(&new_file.file_path)?;
            let mut file_contents: Vec<u8> = Vec::with_capacity(new_file.file_path.to_string_lossy().len() + 5);
            disk_file.read_to_end(&mut file_contents)?;
            map_name = String::from_utf8(file_contents)?;
        }

        println!("Trying to open shared memory \"{}\"", map_name);

        let mut meta: MemMetadata = MemMetadata {
            owner: new_file.owner,
            map_name: Some(map_name),
            map_fd: None,
            map_ctl: None,
            map_size: None,
            map_data: None,
        };


        //Get permissions for the file
        let mut os_perms: OFlag = OFlag::empty();
        let mut file_mode: Mode = Mode::S_IRUSR;

        if new_file.mem_perm.write {
            os_perms.insert(OFlag::O_RDWR);
            file_mode.insert(Mode::S_IWUSR)
        } else {
            os_perms.insert(OFlag::O_RDONLY);
        }

        //Open shared memory
        println!("shm_open()");
        let my_fd: RawFd = shm_open(meta.map_name.as_ref().unwrap().as_str(), os_perms, file_mode)?;
        let file_stat: FileStat = match fstat(my_fd) {
            Ok(v) => v,
            Err(e) => {
                //Close fd if we cant find the size...
                unsafe {match close(my_fd) {_=>{},}};
                return Err(From::from(e));
            }
        };

        meta.map_fd = Some(my_fd);
        let actual_size: usize = file_stat.st_size as usize;
        new_file.mem_size = actual_size - size_of::<MemCtl>();

        println!("shm_open of size 0x{:x}", file_stat.st_size);

        let mut prot_flags: ProtFlags = ProtFlags::empty();
        if new_file.mem_perm.read {prot_flags.insert(ProtFlags::PROT_READ)}
        if new_file.mem_perm.write {prot_flags.insert(ProtFlags::PROT_WRITE)}
        if new_file.mem_perm.execute {prot_flags.insert(ProtFlags::PROT_EXEC)}

        let mut map_flags: MapFlags = MapFlags::empty();
        map_flags.insert(MapFlags::MAP_SHARED);

        println!("mmap()");
        //Map memory into our address space
        let map_addr: *mut c_void = unsafe {
            mmap(null_mut(), //Desired addr
                actual_size, //size of mapping
                prot_flags, //Permissions on pages
                map_flags, //What kind of mapping
                meta.map_fd.unwrap(), //fd
                0   //Offset into fd
            )
        }?;


        //Initialise our metadata struct
        {
            //Create control structures for the mapping
            meta.map_ctl = Some(map_addr as *mut _);
            //Save the actual size of the mapping
            meta.map_size = Some(actual_size);
            //Init pointer to user data
            meta.map_data = Some((map_addr as usize + size_of::<MemCtl>()) as *mut c_void);


            println!("Openned mapping of size 0x{:x} !", meta.map_size.unwrap());
            println!("MetaHeader @ {:p}", meta.map_ctl.unwrap());
            println!("Data @ {:p}", meta.map_data.unwrap());

            new_file.meta = Some(meta);
        }



        Ok(new_file)
    }

    pub fn os_create(mut new_file: MemFile) -> Result<MemFile> {

        let mut disk_file = File::create(&new_file.file_path)?;
        println!("File created !");
        if !new_file.file_path.is_file() {
            return Err(From::from("Failed to create file"));
        }

        let mut meta: MemMetadata = MemMetadata {
            owner: new_file.owner,
            map_name: None,
            map_fd: None,
            map_ctl: None,
            map_size: None,
            map_data: None,
        };

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

        meta.map_name = Some(unique_name);

        //Get permissions for the file
        let mut os_perms: OFlag = OFlag::empty();
        let mut file_mode: Mode = Mode::S_IRUSR;

        os_perms.insert(OFlag::O_EXCL);
        os_perms.insert(OFlag::O_CREAT);
        if new_file.mem_perm.write {
            os_perms.insert(OFlag::O_RDWR);
            file_mode.insert(Mode::S_IWUSR)
        } else {
            os_perms.insert(OFlag::O_RDONLY);
        }

        //Create shared memory
        println!("shm_open()");
        meta.map_fd = Some(shm_open(meta.map_name.as_ref().unwrap().as_str(), os_perms, file_mode)?);

        //increase size to requested size + meta
        let actual_size: usize = new_file.mem_size + size_of::<MemCtl>();
        println!("ftruncate(_,{});", actual_size);
        ftruncate(meta.map_fd.unwrap(), actual_size as i64)?;

        let mut prot_flags: ProtFlags = ProtFlags::empty();
        if new_file.mem_perm.read {prot_flags.insert(ProtFlags::PROT_READ)}
        if new_file.mem_perm.write {prot_flags.insert(ProtFlags::PROT_WRITE)}
        if new_file.mem_perm.execute {prot_flags.insert(ProtFlags::PROT_EXEC)}

        let mut map_flags: MapFlags = MapFlags::empty();
        map_flags.insert(MapFlags::MAP_SHARED);

        println!("mmap()");
        //Map memory into our address space
        let map_addr: *mut c_void = unsafe {
            mmap(null_mut(), //Desired addr
                actual_size, //size of mapping
                prot_flags, //Permissions on pages
                map_flags, //What kind of mapping
                meta.map_fd.unwrap(), //fd
                0   //Offset into fd
            )
        }?;

        //Initialise our metadata struct
        {
            //Create control structures for the mapping
            meta.map_ctl = Some(map_addr as *mut _);
            //Save the actual size of the mapping
            meta.map_size = Some(actual_size);
            //Init RwLock
            unsafe{
                pthread_rwlock_init(meta.get_rwlock().unwrap(), null_mut());
            }
            //Init pointer to user data
            meta.map_data = Some((map_addr as usize + size_of::<MemCtl>()) as *mut c_void);


            println!("Created mapping of size 0x{:x} !", meta.map_size.unwrap());
            println!("MetaHeader @ {:p}", meta.map_ctl.unwrap());
            println!("Data @ {:p}", meta.map_data.unwrap());

            new_file.meta = Some(meta);
        }

        //Write unique shmem name to disk
        match disk_file.write(&new_file.meta.as_ref().unwrap().map_name.as_ref().unwrap().as_bytes()) {
            Ok(write_sz) => if write_sz != new_file.meta.as_ref().unwrap().map_name.as_ref().unwrap().as_bytes().len() {
                return Err(From::from("Failed to write full contents info on disk"));
            },
            Err(_) => return Err(From::from("Failed to write info on disk")),
        };

        Ok(new_file)
    }
}

/*
impl std::io::Read for MemFile {

}

impl std::io::Write for MemFile {

}

impl std::io::Seek for MemFile {

}
*/
