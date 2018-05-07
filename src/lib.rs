//! A user friendly crate that allows you to share memory between __processes__
//!
//! ## Examples
//! Creator based on examples/create.rs
//! ```
//! //Create a SharedMem at `pwd`\shared_mem.link that links to a shared memory mapping of size 4096 and managed by a mutex.
//! let mut my_shmem: SharedMem = match SharedMem::create(PathBuf::from("shared_mem.link") LockType::Mutex, 4096).unwrap();
//! //Set explicit scope for the lock (no need to call drop(shared_data))
//! {
//!     //Acquire write lock
//!     let mut shared_data = match my_shmem.wlock_as_slice::<u8>().unwrap();
//!     let src = b"Some string you want to share\x00";
//!     //Write to the shared memory
//!     shared_data[0..src.len()].copy_from_slice(src);
//! }
//! ```
//!
//! Slave based on examples/open.rs
//! ```
// Open an existing SharedMem from `pwd`\shared_mem.link
//! let mut my_shmem: SharedMem = match SharedMem::open(PathBuf::from("shared_mem.link")).unwrap();
//! //Set explicit scope for the lock (no need to call drop(shared_data))
//! {
//!     //Acquire read lock
//!     let mut shared_data = match my_shmem.rlock_as_slice::<u8>().unwrap();
//!     //Print the content of the shared memory as chars
//!     for byte in &shared_data[0..256] {
//!         if *byte == 0 { break; }
//!         print!("{}", *byte as char);
//!     }
//! }
//! ```

#[macro_use]
extern crate cfg_if;

//Load up the proper implementations
cfg_if! {
    if #[cfg(target_os="windows")] {
        mod win;
        use win as os_impl;
    } else if #[cfg(any(target_os="freebsd", target_os="linux", target_os="macos"))] {
        mod nix;
        use nix as os_impl;
    } else {
        compile_error!("This library isnt implemented for this platform...");
    }
}

//Include definitions from locking.rs
mod locking;
pub use locking::*;

use std::path::PathBuf;
use std::fs::{File};
use std::io::{Write, Read};
use std::fs::remove_file;
use std::slice;
use std::os::raw::c_void;
use std::ptr::null_mut;
use std::mem::size_of;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

struct MetaDataHeader {
    user_size: usize,
    num_locks: usize,
    num_events: usize,
}
struct LockHeader {
    lock_id: u8,
    start_ind: usize,
    end_ind: usize,
}
struct EventHeader {
    event_id: u8,
}

//Holds information about the mapping
pub struct SharedMemConf<'a> {
    owner: bool,
    link_path: PathBuf,
    size: usize,
    //read_only: bool,

    lock_data: Vec<GenericLock<'a>>,
    event_data: Vec<GenericEvent>,
    features_size: usize,
}
impl<'a> SharedMemConf<'a> {

    pub fn valid_lock_range(map_size: usize, start_ind: usize, end_ind:usize) -> bool {
        //Validate indexes
        if start_ind == end_ind {
            if start_ind == 0 {
                return true;
            } else {
                return false;
            }
        } else if start_ind > end_ind {
            return false;
        } else if end_ind > map_size {
            return false;
        }

        return true;
    }

    //Returns an initialized SharedMemConf
    pub fn new(new_link_path: PathBuf, map_size: usize) -> SharedMemConf<'a> {
        SharedMemConf {
            owner: false,
            link_path: new_link_path,
            size: map_size,
            //read_only: false,
            lock_data: Vec::with_capacity(2),
            event_data: Vec::with_capacity(2),
            features_size: size_of::<MetaDataHeader>() + size_of::<LockHeader>() + size_of::<EventHeader>(),
        }
    }

    //Adds a lock of specified type on the specified byte indexes to the config
    pub fn add_lock(mut self, lock_type: LockType, start_ind: usize, end_ind: usize) -> Result<SharedMemConf<'a>> {

        if !SharedMemConf::valid_lock_range(self.size, start_ind, end_ind) {
            return Err(From::from("Invalid lock range"));
        }

        //TODO : Validate that this lock doesnt overlap data covered by another lock ?

        let new_lock = GenericLock {
            uid: (lock_type as u8),
            size: os_impl::locktype_size(&lock_type),
            ptr: null_mut(),
            start_ind: start_ind,
            end_ind: end_ind,
            interface: os_impl::lockimpl_from_type(&lock_type),
        };

        //Add the size of this lock to of conf in mem size
        self.features_size += size_of::<LockHeader>() + new_lock.size;

        //Add this lock to our config
        self.lock_data.push(new_lock);

        Ok(self)
    }
    pub fn get_user_size(&self) -> &usize {
        return &self.size;
    }
    pub fn get_metadata_size(&self) -> &usize {
        return &self.features_size;
    }

    //Creates a shared memory mapping from the config
    pub fn create(mut self) -> Result<SharedMem<'a>> {

        //Create link file asap
        let mut cur_link: File;
        if self.link_path.is_file() {
            return Err(From::from("Cannot create SharedMem because file already exists"));
        } else {
            cur_link = File::create(&self.link_path)?;
            self.owner = true;
        }

        let some_str: String = String::from("test_mapping");

        //Create the file mapping
        let os_map: os_impl::MapData = os_impl::create_mapping(&some_str, self.features_size + self.size)?;

        let mut cur_ptr = os_map.map_ptr as usize;

        //Initialize meta data
        let meta_header: &mut MetaDataHeader = unsafe{&mut (*(cur_ptr as *mut MetaDataHeader))};
        meta_header.user_size = self.size;
        meta_header.num_locks = self.lock_data.len();
        meta_header.num_events = self.event_data.len();
        cur_ptr += size_of::<MetaDataHeader>();

        //Initialize locks
        for lock in &mut self.lock_data {
            //Set lock header
            let lock_header: &mut LockHeader = unsafe{&mut (*(cur_ptr as *mut LockHeader))};
            lock_header.lock_id = lock.uid;
            lock_header.start_ind = lock.start_ind;
            lock_header.end_ind = lock.end_ind;
            cur_ptr += size_of::<LockHeader>();
            //Set lock pointer
            lock.ptr = cur_ptr as *mut c_void;
            cur_ptr += lock.size;

            //Initialize the lock
            lock.interface.init(lock, true)?;
        }

        //Initialize events
        for event in &mut self.event_data {
            //Set lock header
            let event_header: &mut EventHeader = unsafe{&mut (*(cur_ptr as *mut EventHeader))};
            event_header.event_id = event.uid;
            cur_ptr += size_of::<EventHeader>();
            //Set lock pointer
            event.ptr = cur_ptr as *mut c_void;
            cur_ptr += event.size;

            //Initialize the event
            //TODO : event.interface.init(event)?;
        }

        match cur_link.write(some_str.as_bytes()) {
            Ok(write_sz) => if write_sz != some_str.as_bytes().len() {
                return Err(From::from("Failed to write full contents info on disk"));
            },
            Err(_) => return Err(From::from("Failed to write info on disk")),
        };

        Ok(SharedMem {
            conf: self,
            os_data: os_map,
            link_file: cur_link,
            data_ptr: cur_ptr as *mut c_void,
        })
    }
}

///Struct used to manipulate the shared memory
pub struct SharedMem<'a> {
    //Config that describes this mapping
    conf: SharedMemConf<'a>,
    //The currently in use link file
    link_file: File,
    //Os specific data for the mapping
    os_data: os_impl::MapData,
    //Pointer to the usable shared memory
    data_ptr: *mut c_void,
}
impl<'a> Drop for SharedMem<'a> {

    ///Deletes the SharedMemConf artifacts
    fn drop(&mut self) {

        //Close the openned link file
        drop(&self.link_file);

        //Delete link file if we own it
        if self.conf.owner {
            if self.conf.link_path.is_file() {
                match remove_file(&self.conf.link_path) {_=>{},};
            }
        }
    }
}

impl<'a> SharedMem<'a> {

    /*
    /// Creates a new SharedMem
    ///
    /// This involves creating a "link" on disk specified by the first parameter.
    /// This link contains the OS specific identifier to the shared memory. The usage of such link files
    /// on disk help manage identifier colisions. (ie: a binary using the same argument to this function
    /// can be ran from different directories without worrying about collisions)
    ///
    /// # Examples
    /// ```
    /// # use shared_memory::*;
    /// # use std::path::PathBuf;
    /// # let mut my_shmem: SharedMem = match SharedMem::open(PathBuf::from("shared_mem.link")) {Ok(v) => v, Err(_) => return,};
    /// //Creates a new shared SharedMem named shared_mem.link of size 4096
    /// let mut my_shmem: SharedMem = match SharedMem::create(PathBuf::from("shared_mem.link"), LockType::Mutex, 4096) {
    ///     Ok(v) => v,
    ///     Err(e) => {
    ///         println!("Error : {}", e);
    ///         println!("Failed to create SharedMem...");
    ///         return;
    ///     }
    /// };
    /// ```
    pub fn create(new_link_path: PathBuf, lock_type: LockType, size: usize) -> Result<SharedMem<'a>> {

        let mut cur_link;
        if new_link_path.is_file() {
            return Err(From::from("Cannot create SharedMem because file already exists"));
        } else {
            cur_link = File::create(&new_link_path)?;
        }

        let my_shmem: SharedMem = SharedMem {
            meta: None,
            owner: true,
            link_path: Some(new_link_path),
            real_path: None,
            size: size,
        };

        let created_file = os_impl::create(my_shmem, lock_type)?;

        //Write OS specific identifier in link file
        if let Some(ref real_path) = created_file.real_path {
            match cur_link.write(real_path.as_bytes()) {
                Ok(write_sz) => if write_sz != real_path.as_bytes().len() {
                    return Err(From::from("Failed to write full contents info on disk"));
                },
                Err(_) => return Err(From::from("Failed to write info on disk")),
            };
        } else {
            panic!("os_impl::create() returned succesfully but didnt update SharedMem::real_path() !");
        }

        Ok(created_file)
    }
    */
    ///Opens an existing SharedMem
    ///
    /// This function takes a path to a link file created by create().
    /// Open() will automatically detect the size and locking mechanisms.
    ///
    /// # Examples
    /// ```
    /// use shared_memory::*;
    /// //Opens an existing shared SharedMem named test.txt
    /// let mut my_shmem: SharedMem = match SharedMem::open(PathBuf::from("shared_mem.link")) {
    ///     Ok(v) => v,
    ///     Err(e) => {
    ///         println!("Error : {}", e);
    ///         println!("Failed to open SharedMem...");
    ///         return;
    ///     }
    /// };
    /// ```
    pub fn open(existing_link_path: PathBuf) -> Result<SharedMem<'a>> {

        // Make sure the link file exists
        if !existing_link_path.is_file() {
            return Err(From::from("Cannot open SharedMem, link file doesnt exists"));
        }

        //Get real_path from link file
        let mut cur_link = File::open(&existing_link_path)?;
        let mut file_contents: Vec<u8> = Vec::with_capacity(existing_link_path.to_string_lossy().len() + 5);
        cur_link.read_to_end(&mut file_contents)?;
        let real_path: String = String::from_utf8(file_contents)?;

        //Attempt to open the mapping
        let os_map = os_impl::open_mapping(&real_path)?;

        //Initialize meta data
        let mut cur_ptr = os_map.map_ptr as usize;
        let max_ptr = cur_ptr + os_map.map_size;

        //Read header for basic info
        let meta_header: &mut MetaDataHeader = unsafe{&mut (*(cur_ptr as *mut MetaDataHeader))};
        let mut map_conf: SharedMemConf = SharedMemConf {
            owner: false,
            link_path: existing_link_path,
            size: meta_header.user_size,
            //read_only: false,
            lock_data: Vec::with_capacity(meta_header.num_locks),
            event_data: Vec::with_capacity(meta_header.num_events),
            features_size: size_of::<MetaDataHeader>() + size_of::<LockHeader>() + size_of::<EventHeader>()
        };
        cur_ptr += size_of::<MetaDataHeader>();

        println!("Openned map with:\n\tSize : {}\n\tNum locks : {}\n\tNum Events : {}"
        , meta_header.user_size, meta_header.num_locks, meta_header.num_events);

        //Basic size check
        if os_map.map_size < (map_conf.size + map_conf.features_size)  {
            return Err(From::from(
                format!("Shared memory header contains an invalid mapping size : (map_size: {}, user_size: {}, meta_size: {})",
                    os_map.map_size,
                    map_conf.size,
                    map_conf.features_size)
            ));
        }

        for _i in 0..meta_header.num_locks {
            if cur_ptr >= max_ptr {
                return Err(From::from("Shared memory metadata is invalid... Not enough space for locks"));
            }
            let lock_header: &mut LockHeader = unsafe{&mut (*(cur_ptr as *mut LockHeader))};
            cur_ptr += size_of::<LockHeader>();

            //Try to figure out the lock type from the given ID
            let lock_type: LockType = lock_uid_to_type(&lock_header.lock_id)?;

            println!("\tFound new lock \"{:?}\" : {}-{}", lock_type, lock_header.start_ind, lock_header.end_ind);

            //Make sure the lock range makes sense
            if !SharedMemConf::valid_lock_range(map_conf.size, lock_header.start_ind, lock_header.end_ind) {
                return Err(From::from("Invalid lock range"));
            }

            let mut new_lock = GenericLock {
                uid: lock_type as u8,
                size: os_impl::locktype_size(&lock_type),
                ptr: cur_ptr as *mut c_void,
                start_ind: lock_header.start_ind,
                end_ind: lock_header.end_ind,
                interface: os_impl::lockimpl_from_type(&lock_type),
            };
            cur_ptr += new_lock.size;

            //Make sure memory is big enough to hold lock data
            if cur_ptr >= max_ptr {
                return Err(From::from("Shared memory metadata is invalid... Not enough space for lock data"));
            }

            //Allow the lock to init itself
            new_lock.interface.init(&mut new_lock, false)?;

            //Save this lock in our conf
            map_conf.lock_data.push(new_lock);
        }

        for _i in 0..meta_header.num_events {
            if cur_ptr >= max_ptr {
                return Err(From::from("Shared memory metadata is invalid... Not enough space for events"));
            }
            let _event_header: &mut EventHeader = unsafe{&mut (*(cur_ptr as *mut EventHeader))};
            cur_ptr += size_of::<EventHeader>();

            //TODO : Init events here
        }

        Ok(SharedMem {
            conf: map_conf,
            os_data: os_map,
            link_file: cur_link,
            data_ptr: cur_ptr as *mut c_void,
        })
    }

    ///Returns the size of the SharedMem
    pub fn get_size(&self) -> &usize {
        &self.conf.size
    }
    ///Returns the link_path of the SharedMem
    pub fn get_link_path(&self) -> &PathBuf {
        &self.conf.link_path
    }
    ///Returns the OS specific path of the shared memory object
    ///
    /// Usualy on Linux, this will point to a file under /dev/shm/
    ///
    /// On Windows, this returns a namespace
    pub fn get_real_path(&self) -> &String {
        &self.os_data.unique_id
    }
}

/// Read [WARNING](trait.SharedMemCast.html#warning) before use
///
/// Trait used to indicate that a type can be cast over the shared memory.
///
/// For now, shared_memory implements the trait on almost all primitive types.
///
/// ### __<span style="color:red">WARNING</span>__
///
/// Only implement this trait if you understand the implications of mapping Rust types to shared memory.
/// When doing so, you should be mindful of :
/// * Does my type have any pointers in its internal representation ?
///    * This is important because pointers in your type need to also point to the shared memory for it to be usable by other processes
/// * Can my type resize its contents ?
///    * If so, the type probably cannot be safely used over shared memory because your type might call alloc/realloc/free on shared memory addresses
/// * Does my type allow for initialisation after instantiation ?
///    * A [R|W]lock to the shared memory returns a reference to your type. That means that any use of that reference assumes that the type was properly initialized.
///
/// An example of a type that __shouldnt__ be cast to the shared memory would be Vec.
/// Vec internaly contains a pointer to a slice containing its data and some other metadata.
/// This means that to cast a Vec to the shared memory, the memory has to already be initialized with valid pointers and metadata.
/// Granted we could initialize those fields manually, the use of the vector might then trigger a free/realloc on our shared memory.
///
/// # Examples
/// ```
/// struct SharedState {
///     num_listenners: u32,
///     message: [u8; 256],
/// }
/// //WARNING : Only do this if you know what you're doing.
/// unsafe impl SharedMemCast for SharedState {}
///
/// <...>
///
/// {
///     let mut shared_state: WriteLockGuard<SharedState> = match my_shmem.wlock().unwrap();
///     shared_state.num_listenners = 0;
///     let src = b"Welcome, we currently have 0 listenners !\x00";
///     shared_state.message[0..src.len()].copy_from_slice(src);
/// }
///```
pub unsafe trait SharedMemCast {}
unsafe impl SharedMemCast for bool {}
unsafe impl SharedMemCast for char {}
unsafe impl SharedMemCast for str {}
unsafe impl SharedMemCast for i8 {}
unsafe impl SharedMemCast for i16 {}
unsafe impl SharedMemCast for i32 {}
unsafe impl SharedMemCast for u8 {}
unsafe impl SharedMemCast for i64 {}
unsafe impl SharedMemCast for u16 {}
unsafe impl SharedMemCast for u64 {}
unsafe impl SharedMemCast for isize {}
unsafe impl SharedMemCast for u32 {}
unsafe impl SharedMemCast for usize {}
unsafe impl SharedMemCast for f32 {}
unsafe impl SharedMemCast for f64 {}
