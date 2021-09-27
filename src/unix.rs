use ::nix::fcntl::OFlag;
use ::nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, MapFlags, ProtFlags};
use ::nix::sys::stat::{fstat, Mode};
use ::nix::unistd::{close, ftruncate};
use sysinfo::{System, SystemExt};

const MEMORY_THRESHOLD: f32 = 0.2;

#[allow(unused_imports)]
use crate::log::*;
use crate::ShmemError;

use std::os::unix::io::RawFd;
use std::ptr::null_mut;

pub struct MapData {
    droppable: bool,
    //On linux, you must shm_unlink() the object created for the mapping. It wont disappear automatically.
    owner: bool,

    //File descriptor to our open mapping
    pub map_fd: RawFd,

    //Shared mapping uid
    pub unique_id: String,
    //Total size of the mapping
    pub map_size: usize,
    //Pointer to the first address of our mapping
    pub map_ptr: usize,
}

/// Shared memory teardown for linux
impl Drop for MapData {
    ///Takes care of properly closing the `SharedMem` (`munmap()`, `shmem_unlink()`, `close()`)
    fn drop(&mut self) {
        if !self.droppable {
            return;
        }
        //Unmap memory
        close_mapping(self);

        //Unlink shmem
        if self.map_fd != 0 {
            //unlink shmem if we created it
            if self.owner {
                // TODO remove?
                debug!("Deleting persistent mapping");
                trace!("shm_unlink({})", self.unique_id.as_str());
                if let Err(_e) = shm_unlink(self.unique_id.as_str()) {
                    debug!("Failed to shm_unlink() shared memory : {}", _e);
                };
            }

            trace!("close({})", self.map_fd);
            if let Err(_e) = close(self.map_fd) {
                debug!(
                    "os_impl::Linux : Failed to close() shared memory file descriptor : {}",
                    _e
                );
            };
        }
    }
}

impl MapData {
    pub fn set_owner(&mut self, is_owner: bool) -> bool {
        let prev_val = self.owner;
        self.owner = is_owner;
        prev_val
    }
}

/// Checks there's available space for the memory allocation, allowing some threshold to avoid using 100%
pub fn check_available_space(map_size: usize) -> Result<(), ShmemError> {
    let sys_stats = System::new_all();
    let free_mem_in_kb = sys_stats
        .available_memory()
        .saturating_sub((MEMORY_THRESHOLD * sys_stats.total_memory() as f32) as u64);

    if free_mem_in_kb * 1024 < map_size as u64 {
        return Err(ShmemError::DevShmOutOfMemory);
    }
    Ok(())
}

/// Creates a mapping specified by the uid and size
pub fn create_mapping(unique_id: &str, map_size: usize, droppable: bool) -> Result<MapData, ShmemError> {
    //Create shared memory file descriptor
    debug!("Creating persistent mapping at {}", unique_id);
    let shmem_fd = match shm_open(
        unique_id, //Unique name that usualy pops up in /dev/shm/
        OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_RDWR, //create exclusively (error if collision) and read/write to allow resize
        Mode::S_IRUSR | Mode::S_IWUSR,                  //Permission allow user+rw
    ) {
        Ok(v) => {
            trace!(
                "shm_open({}, {:X}, {:X}) == {}",
                unique_id,
                OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_RDWR,
                Mode::S_IRUSR | Mode::S_IWUSR,
                v
            );
            v
        }
        Err(nix::Error::EEXIST) => return Err(ShmemError::MappingIdExists),
        Err(e) => return Err(ShmemError::MapCreateFailed(e as u32)),
    };

    let mut new_map: MapData = MapData {
        droppable,
        owner: true,
        unique_id: String::from(unique_id),
        map_fd: shmem_fd,
        map_size,
        map_ptr: 0,
    };

    //Enlarge the memory descriptor file size to the requested map size
    debug!("Creating memory mapping");
    trace!("ftruncate({}, {})", new_map.map_fd, new_map.map_size);
    match ftruncate(new_map.map_fd, new_map.map_size as _) {
        Ok(_) => {}
        Err(e) => return Err(ShmemError::UnknownOsError(e as u32)),
    };

    //Put the mapping in our address space
    debug!("Loading mapping into address space");
    new_map.map_ptr = match unsafe {
        mmap(
            null_mut(),                                   //Desired addr
            new_map.map_size,                             //size of mapping
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE, //Permissions on pages
            MapFlags::MAP_SHARED,                         //What kind of mapping
            new_map.map_fd,                               //fd
            0,                                            //Offset into fd
        )
    } {
        Ok(v) => {
            trace!(
                "mmap(NULL, {}, {:X}, {:X}, {}, 0) == {:p}",
                new_map.map_size,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                new_map.map_fd,
                v
            );
            v as *mut u8 as usize
        }
        Err(e) => return Err(ShmemError::MapCreateFailed(e as u32)),
    };

    Ok(new_map)
}

/// Opens an existing mapping specified by its uid
pub fn open_mapping(unique_id: &str, droppable: bool) -> Result<MapData, ShmemError> {
    //Open shared memory
    debug!("Openning persistent mapping at {}", unique_id);
    let shmem_fd = match shm_open(
        unique_id,
        OFlag::O_RDWR, //Open read write
        Mode::S_IRUSR,
    ) {
        Ok(v) => {
            trace!(
                "shm_open({}, {:X}, {:X}) == {}",
                unique_id,
                OFlag::O_RDWR,
                Mode::S_IRUSR,
                v
            );
            v
        }
        Err(e) => return Err(ShmemError::MapOpenFailed(e as u32)),
    };

    let mut new_map: MapData = MapData {
        droppable,
        owner: false,
        unique_id: String::from(unique_id),
        map_fd: shmem_fd,
        map_size: 0,
        map_ptr: 0,
    };

    //Get mmap size
    new_map.map_size = match fstat(new_map.map_fd) {
        Ok(v) => v.st_size as usize,
        Err(e) => return Err(ShmemError::MapOpenFailed(e as u32)), 
    };

    //Map memory into our address space
    debug!("Loading mapping into address space");
    new_map.map_ptr = match unsafe {
        mmap(
            null_mut(),                                   //Desired addr
            new_map.map_size,                           //size of mapping
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE, //Permissions on pages
            MapFlags::MAP_SHARED,                        //What kind of mapping
            new_map.map_fd,                                    //fd
            0,                                          //Offset into fd
        )
    } {
        Ok(v) => {
            trace!(
                "mmap(NULL, {}, {:X}, {:X}, {}, 0) == {:p}",
                new_map.map_size,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                new_map.map_fd,
                v
            );
            v as *mut u8 as usize
        }
        Err(e) => return Err(ShmemError::MapOpenFailed(e as u32)),
    };

    Ok(new_map)
}

pub fn close_mapping(map_data: &mut MapData) {
    if !(map_data.map_ptr as *const u8).is_null() {
        trace!(
            "munmap(map_ptr:{:p},map_size:{})",
            self.map_ptr,
            self.map_size
        );
        if let Err(_e) = unsafe { munmap(map_data.map_ptr as *mut _, map_data.map_size) } {
            debug!("Failed to munmap() shared memory mapping : {}", _e);
        };
    }
}

pub fn resize_segment(map_data: &mut MapData, new_size: usize) -> Result<(), ShmemError> {
    if new_size > map_data.map_size {
        let size_increase = new_size - map_data.map_size;
        check_available_space(size_increase)?;
    }

    //Enlarge the memory descriptor file size to the requested map size
    match ftruncate(map_data.map_fd, new_size as _) {
        Ok(_) => {}
        Err(e) => return Err(ShmemError::UnknownOsError(e as u32))
    };

    Ok(())
}

pub fn reload_mapping(map_data: &mut MapData) -> Result<(), ShmemError> {
    // TODO: use mremap for a single sys-call and better safety
    let old_map_size = map_data.map_size;
    let desired_address = if (map_data.map_ptr as *const u8).is_null() {
        null_mut()
    } else {
        match unsafe { munmap((map_data.map_ptr as *mut u8) as *mut _, old_map_size) } {
            Ok(_) => {}
            Err(e) => return Err(ShmemError::UnmapFailed(e)),
        };
        (map_data.map_ptr as *mut u8) as *mut _
    };

    map_data.map_size = match fstat(map_data.map_fd) {
        Ok(v) => v.st_size as usize,
        Err(e) => return Err(ShmemError::MapOpenFailed(e as u32)),
    };

    // Remap into our address space
    map_data.map_ptr = match unsafe {
        mmap(
            desired_address,                                //Desired addr
            map_data.map_size,                            //size of mapping
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,   //Permissions on pages
            MapFlags::MAP_SHARED,                          //What kind of mapping
            map_data.map_fd,                                     //file descriptor
            0,                                            //Offset inside "file"
        )
    } {
        Ok(v) => v as *mut u8 as usize,
        Err(e) => return Err(ShmemError::MapOpenFailed(e as u32)),
    };

    Ok(())
}