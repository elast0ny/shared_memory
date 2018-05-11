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

mod locking;
mod events;

pub use locking::*;
pub use events::*;

//Load up the proper OS implementation
cfg_if! {
    if #[cfg(target_os="windows")] {
        mod win;
        use win as os_impl;
    } else if #[cfg(any(target_os="freebsd", target_os="linux", target_os="macos"))] {
        mod nix;
        use nix as os_impl;
    } else {
        compile_error!("shared_memory isnt implemented for this platform...");
    }
}

use std::path::PathBuf;
use std::fs::{File};
use std::io::{Write, Read};
use std::fs::remove_file;
use std::slice;
use std::os::raw::c_void;
use std::ptr::null_mut;
use std::mem::size_of;
use std::sync::atomic::*;

extern crate rand;
use rand::Rng;
extern crate theban_interval_tree;
use theban_interval_tree::*;
extern crate memrange;
use memrange::Range;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

//Structs used in the shared memory metadata
struct MetaDataHeader {
    meta_size: usize,
    user_size: usize,
    num_locks: usize,
    num_events: usize,
}
struct LockHeader {
    uid: u8,
    offset: usize,
    length: usize,
}
struct EventHeader {
    event_id: u8,
}

//Configuration for a SharedMem
pub struct SharedMemConf<'a> {
    owner: bool,
    link_path: Option<PathBuf>,
    wanted_os_path: Option<String>,
    size: usize,

    meta_size: usize,
    lock_range_tree: IntervalTree<usize>,
    lock_data: Vec<GenericLock<'a>>,
    event_data: Vec<GenericEvent>,
}
impl<'a> SharedMemConf<'a> {

    pub fn valid_lock_range(map_size: usize, offset: usize, length:usize) -> bool {

        // If lock doesnt protect memory, offset must be 0
        if length == 0 {
            if offset != 0  {
                return false;
            } else {
                return true;
            }
        }

        if offset + (length - 1) >= map_size {
            return false;
        }

        return true;
    }

    //Returns an initialized SharedMemConf
    pub fn new() -> SharedMemConf<'a> {
        SharedMemConf {
            owner: false,
            link_path: None,
            wanted_os_path: None,
            size: 0,
            //read_only: false,
            lock_range_tree: IntervalTree::<usize>::new(),
            lock_data: Vec::with_capacity(2),
            event_data: Vec::with_capacity(2),
            meta_size: size_of::<MetaDataHeader>(),
        }
    }

    pub fn set_size(mut self, wanted_size: usize) -> SharedMemConf<'a> {
        self.size = wanted_size;
        return self;
    }

    pub fn set_link(mut self, link_path: &PathBuf) -> SharedMemConf<'a> {
        self.link_path = Some(link_path.clone());
        return self;
    }

    pub fn set_os_path(mut self, unique_id: &str) -> SharedMemConf<'a> {
        self.wanted_os_path = Some(String::from(unique_id));
        return self;
    }

    //Adds a lock of specified type on the specified byte indexes to the config
    pub fn add_lock(mut self, lock_type: LockType, offset: usize, length: usize) -> Result<SharedMemConf<'a>> {

        if !SharedMemConf::valid_lock_range(self.size, offset, length) {
            return Err(From::from(format!("Invalid lock range: map size 0x{:x}, lock offset 0x{:x}, lock length 0x{:x}", self.size, offset, length)));
        }

        if length != 0 {
            let start_offset: u64 = offset as u64;
            let end_offset: u64 = offset  as u64 + (length - 1) as u64;

            //Make sure this lock doesnt overlap data from another lock
            if let Some(existing_lock) = self.lock_range_tree.range(start_offset, end_offset).next() {
                return Err(From::from(format!("Lock #{} already covers this range...", existing_lock.1)));
            }

            self.lock_range_tree.insert(Range::new(start_offset, end_offset), self.lock_data.len());
        }

        let new_lock = GenericLock {
            uid: (lock_type as u8),
            offset: offset,
            length: length,
            lock_ptr: null_mut(),
            data_ptr: null_mut(),
            interface: os_impl::lockimpl_from_type(&lock_type),
        };

        //Add the size of this lock to our metadata size
        self.meta_size += size_of::<LockHeader>() + new_lock.interface.size_of();

        //Add this lock to our config
        self.lock_data.push(new_lock);

        Ok(self)
    }
    pub fn get_user_size(&self) -> &usize {
        return &self.size;
    }
    pub fn get_metadata_size(&self) -> &usize {
        return &self.meta_size;
    }

    //Creates a shared memory mapping from the config
    pub fn create(mut self) -> Result<SharedMem<'a>> {

        //Create link file asap
        let mut cur_link: Option<File> = None;
        if let Some(ref file_path) = self.link_path {
            if file_path.is_file() {
                return Err(From::from("Cannot create SharedMem because file already exists"));
            } else {
                cur_link = Some(File::create(file_path)?);
                self.owner = true;
            }
        }

        let unique_id: String = match self.wanted_os_path {
            Some(ref s) => s.clone(),
            None => {
                format!("shmem_rs_{:16X}", rand::thread_rng().gen::<u64>())
            },
        };

        println!("Trying to open \"{}\" len {}", unique_id, unique_id.len());

        //Create the file mapping
        let os_map: os_impl::MapData = os_impl::create_mapping(&unique_id, self.meta_size + self.size)?;

        let mut cur_ptr = os_map.map_ptr as usize;
        let user_ptr = os_map.map_ptr as usize + self.meta_size;

        //Initialize meta data
        let meta_header: &mut MetaDataHeader = unsafe{&mut (*(cur_ptr as *mut MetaDataHeader))};
        meta_header.meta_size = self.meta_size;
        meta_header.user_size = self.size;
        meta_header.num_locks = self.lock_data.len();
        meta_header.num_events = self.event_data.len();
        cur_ptr += size_of::<MetaDataHeader>();

        //Initialize locks
        for lock in &mut self.lock_data {
            //Set lock header
            let lock_header: &mut LockHeader = unsafe{&mut (*(cur_ptr as *mut LockHeader))};
            lock_header.uid = lock.uid;
            lock_header.offset = lock.offset;
            lock_header.length = lock.length;
            cur_ptr += size_of::<LockHeader>();
            //Set lock pointer
            lock.lock_ptr = cur_ptr as *mut c_void;
            lock.data_ptr = (user_ptr + lock.offset) as *mut c_void;
            cur_ptr += lock.interface.size_of();

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

            //Initialize the event
            //cur_ptr += event.interface.size_of();
            //TODO : event.interface.init(event)?;
        }

        if let Some(ref mut openned_link) =  cur_link {
            match openned_link.write(unique_id.as_bytes()) {
                Ok(write_sz) => if write_sz != unique_id.as_bytes().len() {
                    return Err(From::from("Failed to write full contents info on disk"));
                },
                Err(_) => return Err(From::from("Failed to write info on disk")),
            };
        }

        println!("Created map with:
        MetaSize : {}
        Size : {}
        Num locks : {}
        Num Events : {}
        MetaAddr {:p}
        UserAddr 0x{:x}",
            meta_header.meta_size,
            meta_header.user_size,
            meta_header.num_locks,
            meta_header.num_events,
            os_map.map_ptr,
            user_ptr,
        );

        Ok(SharedMem {
            conf: self,
            os_data: os_map,
            link_file: cur_link,
        })
    }
}

///Wrapper providing locks/events on a shared memory mapping
pub struct SharedMem<'a> {
    //Config that describes this mapping
    conf: SharedMemConf<'a>,
    //The currently in use link file
    link_file: Option<File>,
    //Os specific data for the mapping
    os_data: os_impl::MapData,
}
impl<'a> SharedMem<'a> {

    pub fn create(lock_type: LockType, size: usize) -> Result<SharedMem<'a>> {
        //Create from a simple sharedmemconf with one lock
        SharedMemConf::new()
            .set_size(size)
            .add_lock(lock_type, 0, size).unwrap().create()
    }

    pub fn open(unique_id: &str) -> Result<SharedMem<'a>> {
        //Attempt to open the mapping
        let os_map = os_impl::open_mapping(&unique_id)?;

        if size_of::<MetaDataHeader>() > os_map.map_size {
            return Err(From::from("Mapping is smaller than our metadata header size !"));
        }

        //Initialize meta data
        let mut cur_ptr = os_map.map_ptr as usize;

        //Read header for basic info
        let meta_header: &mut MetaDataHeader = unsafe{&mut (*(cur_ptr as *mut MetaDataHeader))};
        cur_ptr += size_of::<MetaDataHeader>();

        let mut map_conf: SharedMemConf = SharedMemConf {
            owner: false,
            link_path: None,
            wanted_os_path: None,
            size: meta_header.user_size,
            //read_only: false,
            lock_range_tree: IntervalTree::<usize>::new(),
            lock_data: Vec::with_capacity(meta_header.num_locks),
            event_data: Vec::with_capacity(meta_header.num_events),
            meta_size: size_of::<MetaDataHeader>(),
        };

        //Basic size check on (metadata size + userdata size)
        if os_map.map_size < (meta_header.meta_size + meta_header.user_size) {
            return Err(From::from(
                format!("Shared memory header contains an invalid mapping size : (map_size: {}, meta_size: {}, user_size: {})",
                    os_map.map_size,
                    meta_header.user_size,
                    meta_header.meta_size)
            ));
        }

        //Add the metadata size to our base pointer to get user addr
        let user_ptr = os_map.map_ptr as usize + meta_header.meta_size;

        for i in 0..meta_header.num_locks {

            let lock_header: &mut LockHeader = unsafe{&mut (*(cur_ptr as *mut LockHeader))};
            cur_ptr += size_of::<LockHeader>();
            //Make sure address is valid before reading lock header
            if cur_ptr > user_ptr {
                return Err(From::from("Shared memory metadata is invalid... Not enought space to read lock header fields"));
            }

            //Try to figure out the lock type from the given ID
            let lock_type: LockType = lock_uid_to_type(&lock_header.uid)?;

            println!("\tFound new lock \"{:?}\" : offset {} length {}", lock_type, lock_header.offset, lock_header.length);

            //Add new lock to our config
            map_conf = map_conf.add_lock(lock_type, lock_header.offset, lock_header.length)?;

            let new_lock: &mut GenericLock = map_conf.lock_data.last_mut().unwrap();

            new_lock.lock_ptr = cur_ptr as *mut c_void;
            new_lock.data_ptr = (user_ptr + lock_header.offset) as *mut c_void;
            //Allow the lock to init itself as an existing lock
            new_lock.interface.init(new_lock, false)?;

            cur_ptr += new_lock.interface.size_of();
            //Make sure memory is big enough to hold lock data
            if cur_ptr > user_ptr {
                return Err(From::from(
                    format!("Shared memory metadata is invalid... Trying to read lock {} of size 0x{:x} at address 0x{:x} but user data starts at 0x{:x}..."
                        , i, new_lock.interface.size_of(), cur_ptr, user_ptr)
                ));
            }

        }

        for _i in 0..meta_header.num_events {
            if cur_ptr >= user_ptr {
                return Err(From::from("Shared memory metadata is invalid... Not enough space for events"));
            }
            let _event_header: &mut EventHeader = unsafe{&mut (*(cur_ptr as *mut EventHeader))};
            cur_ptr += size_of::<EventHeader>();

            //TODO : Init events here
        }

        if cur_ptr != user_ptr {
            return Err(From::from(format!("Shared memory metadata does not end right before user data ! 0x{:x} != 0x{:x}", cur_ptr, user_ptr)));
        } else if map_conf.meta_size != meta_header.meta_size {
            return Err(From::from(format!("Shared memory metadata does not match what was advertised ! 0x{:x} != 0x{:x}", map_conf.meta_size, meta_header.meta_size)));
        }

        Ok(SharedMem {
            conf: map_conf,
            os_data: os_map,
            link_file: None,
        })
    }

    pub fn create_linked(new_link_path: &PathBuf, lock_type: LockType, size: usize) -> Result<SharedMem<'a>> {
        //Create from a simple sharedmemconf with one lock
        SharedMemConf::new()
            .set_link(new_link_path)
            .set_size(size)
            .add_lock(lock_type, 0, size).unwrap().create()
    }
    pub fn open_link(existing_link_path: PathBuf) -> Result<SharedMem<'a>> {

        // Make sure the link file exists
        if !existing_link_path.is_file() {
            return Err(From::from("Cannot open SharedMem, link file doesnt exists"));
        }

        //Get real_path from link file
        let mut cur_link = File::open(&existing_link_path)?;
        let mut file_contents: Vec<u8> = Vec::with_capacity(existing_link_path.to_string_lossy().len() + 5);
        cur_link.read_to_end(&mut file_contents)?;
        let real_path: String = String::from_utf8(file_contents)?;

        let mut new_shmem = SharedMem::open(&real_path)?;

        //Set the link file info
        new_shmem.conf.link_path = Some(existing_link_path);
        new_shmem.link_file = Some(cur_link);

        return Ok(new_shmem);
    }

    ///Returns the size of the SharedMem
    pub fn get_size(&self) -> &usize {
        &self.conf.size
    }
    ///Returns the link_path of the SharedMem
    pub fn get_link_path(&self) -> &Option<PathBuf> {
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
impl<'a> Drop for SharedMem<'a> {

    ///Deletes the SharedMemConf artifacts
    fn drop(&mut self) {

        //Close the openned link file
        drop(&self.link_file);

        //Delete link file if we own it
        if self.conf.owner {
            if let Some(ref file_path) = self.conf.link_path {
                if file_path.is_file() {
                    match remove_file(file_path) {_=>{},};
                }
            }
        }
    }
}
impl<'a>ReadLockable for SharedMem<'a> {
    fn rlock<D: SharedMemCast>(&self, lock_index: usize) -> Result<ReadLockGuard<D>> {

        let lock: &GenericLock = &self.conf.lock_data[lock_index];

        //Make sure that we can cast our memory to the type
        let type_size = std::mem::size_of::<D>();
        if type_size > lock.length {
            return Err(From::from(
                format!("Tried to map type of {} bytes to a lock holding only {} bytes", type_size, lock.length)
            ));
        }

        //Return data wrapped in a lock
        Ok(
            //Unsafe required to cast shared memory to our type
            unsafe {
                ReadLockGuard::lock(
                    &(*(lock.data_ptr as *const D)),
                    lock.interface,
                    &mut (*lock.lock_ptr),
                )
            }
        )
    }

    fn rlock_as_slice<D: SharedMemCast>(&self, lock_index: usize) -> Result<ReadLockGuardSlice<D>>{

        let lock: &GenericLock = &self.conf.lock_data[lock_index];

        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > lock.length {
            return Err(From::from(
                format!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size, lock.length)
            ));
        }
        let num_items: usize = lock.length / item_size;

        //Return data wrapped in a lock
        Ok(
            //Unsafe required to cast shared memory to array
            unsafe {
                ReadLockGuardSlice::lock(
                    slice::from_raw_parts(lock.data_ptr as *const D, num_items),
                    lock.interface,
                    &mut (*lock.lock_ptr),
                )
            }
        )
    }
}
impl<'a>WriteLockable for SharedMem<'a> {
    fn wlock<D: SharedMemCast>(&mut self, lock_index: usize) -> Result<WriteLockGuard<D>> {

        let lock: &GenericLock = &self.conf.lock_data[lock_index];

        //Make sure that we can cast our memory to the type
        let type_size = std::mem::size_of::<D>();
        if type_size > lock.length {
            return Err(From::from(
                format!("Tried to map type of {} bytes to a lock holding only {} bytes", type_size, lock.length)
            ));
        }

        //Return data wrapped in a lock
        Ok(
            //Unsafe required to cast shared memory to our type
            unsafe {
                WriteLockGuard::lock(
                    &mut (*(lock.data_ptr as *mut D)),
                    lock.interface,
                    &mut (*lock.lock_ptr),
                )
            }
        )
    }

    fn wlock_as_slice<D: SharedMemCast>(&mut self, lock_index: usize) -> Result<WriteLockGuardSlice<D>> {

        let lock: &GenericLock = &self.conf.lock_data[lock_index];

        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > lock.length {
            return Err(From::from(
                format!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size, lock.length)
            ));
        }
        //Calculate how many items our slice will have
        let num_items: usize = lock.length / item_size;

        //Return data wrapped in a lock
        Ok(
            //Unsafe required to cast shared memory to array
            unsafe {
                WriteLockGuardSlice::lock(
                    slice::from_raw_parts_mut((lock.data_ptr as usize + 0) as *mut D, num_items),
                    lock.interface,
                    &mut (*lock.lock_ptr),
                )
            }
        )
    }
}
impl<'a> ReadRaw for SharedMem<'a> {
    unsafe fn get_raw<D: SharedMemCast>(&self) -> &D {
        let user_data = self.os_data.map_ptr as usize + self.conf.meta_size;
        return &(*(user_data as *const D))
    }

    unsafe fn get_raw_slice<D: SharedMemCast>(&self) -> &[D] {
        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > self.conf.size {
            panic!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size, self.conf.size);
        }
        let num_items: usize = self.conf.size / item_size;
        let user_data = self.os_data.map_ptr as usize + self.conf.meta_size;

        return slice::from_raw_parts(user_data as *const D, num_items);
    }
}
impl<'a> WriteRaw for SharedMem<'a> {
    unsafe fn get_raw_mut<D: SharedMemCast>(&mut self) -> &mut D {
        let user_data = self.os_data.map_ptr as usize + self.conf.meta_size;
        return &mut (*(user_data as *mut D))
    }
    unsafe fn get_raw_slice_mut<D: SharedMemCast>(&mut self) -> &mut[D] {
        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > self.conf.size {
            panic!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size, self.conf.size);
        }
        let num_items: usize = self.conf.size / item_size;
        let user_data = self.os_data.map_ptr as usize + self.conf.meta_size;

        return slice::from_raw_parts_mut(user_data as *mut D, num_items);
    }
}

///Raw shared memory mapping
pub struct SharedMemRaw {
    //Os specific data for the mapping
    os_data: os_impl::MapData,
}
impl SharedMemRaw {

    pub fn create(unique_id: String, size: usize) -> Result<SharedMemRaw> {

        let os_map: os_impl::MapData = os_impl::create_mapping(&unique_id, size)?;

        Ok(SharedMemRaw {
            os_data: os_map,
        })
    }
    pub fn open(unique_id: String) -> Result<SharedMemRaw> {

        //Attempt to open the mapping
        let os_map = os_impl::open_mapping(&unique_id)?;

        Ok(SharedMemRaw {
            os_data: os_map,
        })
    }

    ///Returns the size of the SharedMemRaw mapping
    pub fn get_size(&self) -> &usize {
        &self.os_data.map_size
    }
    ///Returns the OS specific path of the shared memory object
    ///
    /// Usualy on Linux, this will point to a "file" under /dev/shm/
    ///
    /// On Windows, this returns a namespace
    pub fn get_path(&self) -> &String {
        &self.os_data.unique_id
    }
}
impl ReadRaw for SharedMemRaw {
    unsafe fn get_raw<D: SharedMemCast>(&self) -> &D {
        return &(*(self.os_data.map_ptr as *const D))
    }

    unsafe fn get_raw_slice<D: SharedMemCast>(&self) -> &[D] {
        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > self.os_data.map_size {
            panic!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size,  self.os_data.map_size);
        }
        let num_items: usize =  self.os_data.map_size / item_size;

        return slice::from_raw_parts(self.os_data.map_ptr as *const D, num_items);
    }
}
impl WriteRaw for SharedMemRaw {
    unsafe fn get_raw_mut<D: SharedMemCast>(&mut self) -> &mut D {
        return &mut (*(self.os_data.map_ptr as *mut D))
    }
    unsafe fn get_raw_slice_mut<D: SharedMemCast>(&mut self) -> &mut[D] {
        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size >  self.os_data.map_size {
            panic!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size,  self.os_data.map_size);
        }
        let num_items: usize =  self.os_data.map_size / item_size;

        return slice::from_raw_parts_mut(self.os_data.map_ptr as *mut D, num_items);
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

unsafe impl SharedMemCast for AtomicBool {}
unsafe impl SharedMemCast for AtomicIsize {}
unsafe impl<T> SharedMemCast for AtomicPtr<T> {}
unsafe impl SharedMemCast for AtomicUsize {}
