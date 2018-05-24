//! A user friendly crate that allows you to share memory between __processes__

#[macro_use]
extern crate cfg_if;

#[macro_use]
extern crate enum_primitive;
pub use enum_primitive::FromPrimitive;

mod lock_defs;
mod event_defs;

pub use lock_defs::*;
pub use event_defs::*;

const ADDR_ALIGN: u8 = 4;

//Load up the proper OS implementation
cfg_if! {
    if #[cfg(target_os="windows")] {
        mod windows;
        use windows as os_impl;
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
use std::fmt;

extern crate rand;
use rand::Rng;
extern crate theban_interval_tree;
use theban_interval_tree::*;
extern crate memrange;
use memrange::Range;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

///Defines different variants to specify timeouts
pub enum Timeout {
    ///Wait forever for an event to be signaled
    Infinite,
    ///Duration in seconds for a timeout
    Sec(usize),
    ///Duration in milliseconds for a timeout
    Milli(usize),
    ///Duration in microseconds for a timeout
    Micro(usize),
    ///Duration in nanoseconds for a timeout
    Nano(usize),
}

//Changes the content of val to the next multiple of align returning the amount that was required to align
fn align_value(val: &mut usize, align: u8) -> u8 {
    let tmp: u8 = align-1;
    let old_val = *val;
    //Make sure our data will be starting on a nice address
    if *val & tmp as usize != 0 {
        *val = (*val + tmp as usize) & !(tmp as usize);
    }

    //Return the amount of padding
    (*val - old_val) as u8
}

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
    uid: u8,
}

///Configuration used to describe a shared memory mapping before its creation
pub struct SharedMemConf<'a> {
    is_live: bool,
    owner: bool,
    link_path: Option<PathBuf>,
    wanted_os_path: Option<String>,
    size: usize,

    meta_size: usize,
    lock_range_tree: IntervalTree<usize>,
    lock_data: Vec<GenericLock<'a>>,
    event_data: Vec<GenericEvent<'a>>,
}
impl<'a> SharedMemConf<'a> {

    //Validate if a lock range makes sense based on the mapping size
    fn valid_lock_range(map_size: usize, offset: usize, length:usize) -> bool {

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
    ///Returns a new SharedMemConf
    pub fn new() -> SharedMemConf<'a> {
        SharedMemConf {
            is_live: false,
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
    ///Sets the size of the usable memory in the mapping
    pub fn set_size(mut self, wanted_size: usize) -> SharedMemConf<'a> {
        self.size = wanted_size;
        return self;
    }
    ///Sets the path for the link file
    pub fn set_link_path(mut self, link_path: &PathBuf) -> SharedMemConf<'a> {
        self.link_path = Some(link_path.clone());
        return self;
    }
    ///Sets a specific unique_id to be used when creating the mapping
    pub fn set_os_path(mut self, unique_id: &str) -> SharedMemConf<'a> {
        self.wanted_os_path = Some(String::from(unique_id));
        return self;
    }
    fn add_lock_impl(&mut self, lock_type: LockType, offset: usize, length: usize) -> Result<()> {
        if !SharedMemConf::valid_lock_range(self.size, offset, length) {
            return Err(From::from(format!(
                "add_lock({:?}, {}, {}) : Invalid lock range for map size {}",
                lock_type, offset, length, self.size)));
        }

        if length != 0 {
            let start_offset: u64 = offset as u64;
            let end_offset: u64 = offset  as u64 + (length - 1) as u64;

            //Make sure this lock doesnt overlap data from another lock
            if let Some(existing_lock) = self.lock_range_tree.range(start_offset, end_offset).next() {
                return Err(From::from(format!(
                    "add_lock({:?}, {}, {}) : Lock #{} already covers this range...",
                    lock_type, offset, length, existing_lock.1)));
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

        Ok(())
    }
    ///Adds a lock of specified type on a range of bytes
    pub fn add_lock(mut self, lock_type: LockType, offset: usize, length: usize) -> Result<SharedMemConf<'a>> {
        self.add_lock_impl(lock_type, offset, length)?;
        Ok(self)
    }
    fn add_event_impl(&mut self, event_type: EventType) -> Result<()> {
        let new_event = GenericEvent {
            uid: (event_type as u8),
            ptr: null_mut(),
            interface: os_impl::eventimpl_from_type(&event_type),
        };

        //Add the size of this lock to our metadata size
        self.meta_size += size_of::<EventHeader>() + new_event.interface.size_of();

        //Add this lock to our config
        self.event_data.push(new_event);

        Ok(())
    }
    ///Adds an event of specified type
    pub fn add_event(mut self, event_type: EventType) -> Result<SharedMemConf<'a>> {
        self.add_event_impl(event_type)?;
        Ok(self)
    }
    ///Calculates the meta data size required given the current config
    pub fn get_metadata_size(&self) -> usize {

        //This is static if the memory has been created
        if self.is_live {
            return self.meta_size;
        }

        let mut meta_size = size_of::<MetaDataHeader>();

        //We must dynamically go through locks&event because
        //padding might have to be added to align data depending
        //On the order the locks&events are int

        for ref lock in &self.lock_data {
            meta_size += size_of::<LockHeader>();
            //Lock data starts at aligned addr
            align_value(&mut meta_size, ADDR_ALIGN);
            meta_size += lock.interface.size_of();

        }
        for ref event in &self.event_data {
            meta_size += size_of::<EventHeader>();
            //Event data starts at aligned addr
            align_value(&mut meta_size, ADDR_ALIGN);
            meta_size += event.interface.size_of();
        }

        //User data starts at an aligned offset also
        align_value(&mut meta_size, ADDR_ALIGN);
        meta_size
    }
    ///Creates a shared memory mapping from a config
    pub fn create(mut self) -> Result<SharedMem<'a>> {

        if self.size == 0 {
            return Err(From::from("SharedMemConf.create() : Cannot create a mapping of size 0"));
        }

        //Create link file if required
        let mut cur_link: Option<File> = None;
        if let Some(ref file_path) = self.link_path {
            if file_path.is_file() {
                return Err(From::from("SharedMemConf.create() : Link file already exists"));
            } else {
                cur_link = Some(File::create(file_path)?);
                self.owner = true;
            }
        }

        //Generate a random unique_id if not specified
        let unique_id: String = match self.wanted_os_path {
            Some(ref s) => s.clone(),
            None => {
                format!("shmem_rs_{:16X}", rand::thread_rng().gen::<u64>())
            },
        };

        let meta_size: usize = self.get_metadata_size();
        //Create the file mapping
        //TODO : Handle unique_id collision if randomly generated
        let os_map: os_impl::MapData = os_impl::create_mapping(&unique_id, meta_size + self.size)?;

        //Write the unique_id of the mapping in the link file
        if let Some(ref mut openned_link) =  cur_link {
            match openned_link.write(unique_id.as_bytes()) {
                Ok(write_sz) => if write_sz != unique_id.as_bytes().len() {
                    return Err(From::from("SharedMemConf.create() : Failed to write unique_id to link file"));
                },
                Err(_) => return Err(From::from("SharedMemConf.create() : Failed to write unique_id to link file")),
            };
        }

        let mut cur_ptr = os_map.map_ptr as usize;
        let user_ptr = os_map.map_ptr as usize + meta_size;

        //Initialize meta data
        let meta_header: &mut MetaDataHeader = unsafe{&mut (*(cur_ptr as *mut MetaDataHeader))};
        //Set the header for our shared memory
        meta_header.meta_size = meta_size;
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
            align_value(&mut cur_ptr, ADDR_ALIGN);

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
            event_header.uid = event.uid;
            cur_ptr += size_of::<EventHeader>();
            align_value(&mut cur_ptr, ADDR_ALIGN);

            //Set lock pointer
            event.ptr = cur_ptr as *mut c_void;

            //Initialize the event
            cur_ptr += event.interface.size_of();
            event.interface.init(event, true)?;
        }

        //Make sure the user data is aligned
        align_value(&mut cur_ptr, ADDR_ALIGN);

        self.meta_size = meta_size;
        self.is_live = true;

        Ok(SharedMem {
            conf: self,
            os_data: os_map,
            user_ptr: cur_ptr as *mut c_void,
            link_file: cur_link,
        })
    }

    pub fn open(mut self) -> Result<SharedMem<'a>> {

        //Attempt to open the mapping
        let mut cur_link: Option<File> = None;

        //Open mapping from explicit os_path or from link file
        let os_map: os_impl::MapData = match self.wanted_os_path {
            Some(ref v) => os_impl::open_mapping(v)?,
            None => {
                //Check if a link file is specified
                if let Some(ref link_file_path) = self.link_path {
                    if !link_file_path.is_file() {
                        return Err(From::from("Cannot find unique os path since link_file does not exist"));
                    }
                    //Get real_path from link file
                    let mut link_file = File::open(link_file_path)?;
                    let mut file_contents: Vec<u8> = Vec::new();
                    link_file.read_to_end(&mut file_contents)?;
                    cur_link = Some(link_file);
                    os_impl::open_mapping(&String::from_utf8(file_contents)?)?
                } else {
                    return Err(From::from("Cannot find unique os path since link_file is not set"));
                }
            }
        };
        self.owner = false;

        if size_of::<MetaDataHeader>() > os_map.map_size {
            return Err(From::from("Mapping is smaller than our metadata header size !"));
        }

        //Initialize meta data
        let mut cur_ptr = os_map.map_ptr as usize;

        //Read header for basic info
        let meta_header: &mut MetaDataHeader = unsafe{&mut (*(cur_ptr as *mut MetaDataHeader))};
        cur_ptr += size_of::<MetaDataHeader>();

        self.size = meta_header.user_size;

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

        //Open&initialize all locks
        for i in 0..meta_header.num_locks {

            let lock_header: &mut LockHeader = unsafe{&mut (*(cur_ptr as *mut LockHeader))};
            cur_ptr += size_of::<LockHeader>();
            align_value(&mut cur_ptr, ADDR_ALIGN);

            //Make sure address is valid before reading lock header
            if cur_ptr > user_ptr {
                return Err(From::from("Shared memory metadata is invalid... Not enought space to read lock header fields"));
            }

            //Try to figure out the lock type from the given ID
            let lock_type: LockType = match LockType::from_u8(lock_header.uid) {
                Some(t) => t,
                None => {
                    return Err(From::from(format!("Shared memory metadata contained invalid lock uid {}", lock_header.uid)));
                }
            };

            println!("\tFound new lock \"{:?}\" : offset {} length {}", lock_type, lock_header.offset, lock_header.length);

            //Add new lock to our config
            self.add_lock_impl(lock_type, lock_header.offset, lock_header.length)?;

            let new_lock: &mut GenericLock = self.lock_data.last_mut().unwrap();

            new_lock.lock_ptr = cur_ptr as *mut c_void;
            new_lock.data_ptr = (user_ptr + lock_header.offset) as *mut c_void;

            cur_ptr += new_lock.interface.size_of();
            //Make sure memory is big enough to hold lock data
            if cur_ptr > user_ptr {
                return Err(From::from(
                    format!("Shared memory metadata is invalid... Trying to read lock {} of size 0x{:x} at address 0x{:x} but user data starts at 0x{:x}..."
                        , i, new_lock.interface.size_of(), cur_ptr, user_ptr)
                ));
            }

            //Allow the lock to init itself as an existing lock
            new_lock.interface.init(new_lock, false)?;
        }

        //Open&initialize all events
        for i in 0..meta_header.num_events {

            let event_header: &mut EventHeader = unsafe{&mut (*(cur_ptr as *mut EventHeader))};
            cur_ptr += size_of::<EventHeader>();
            align_value(&mut cur_ptr, ADDR_ALIGN);

            if cur_ptr > user_ptr {
                return Err(From::from("Shared memory metadata is invalid... Not enough space for events"));
            }

            let event_type: EventType = match EventType::from_u8(event_header.uid) {
                Some(t) => t,
                None => {
                    return Err(From::from(format!("Shared memory metadata contained invalid event uid {}", event_header.uid)));
                }
            };

            println!("\tFound new event \"{:?}\"", event_type);

            self.add_event_impl(event_type)?;

            let new_event: &mut GenericEvent = self.event_data.last_mut().unwrap();

            //If event has no data in shared memory, early exit
            if new_event.interface.size_of() == 0 {
                new_event.interface.init(new_event, false)?;
                continue;
            }
            new_event.ptr = cur_ptr as *mut c_void;
            cur_ptr += new_event.interface.size_of();

            //Make sure memory is big enough to hold lock data
            if cur_ptr > user_ptr {
                return Err(From::from(
                    format!("Shared memory metadata is invalid... Trying to read event {} of size 0x{:x} at address 0x{:x} but user data starts at 0x{:x}..."
                        , i, new_event.interface.size_of(), cur_ptr, user_ptr)
                ));
            }

            //Allow the lock to init itself as an existing lock
            new_event.interface.init(new_event, false)?;
        }

        //User data is supposed to be aligned
        align_value(&mut cur_ptr, ADDR_ALIGN);

        //Get the metadata size that we calculated while parsing the header
        self.meta_size = cur_ptr - os_map.map_ptr as usize;

        if cur_ptr != user_ptr {
            return Err(From::from(format!("Shared memory metadata does not end right before user data ! 0x{:x} != 0x{:x}", cur_ptr, user_ptr)));
        } else if self.meta_size != meta_header.meta_size {
            return Err(From::from(format!("Shared memory metadata does not match what was advertised ! {} != {}", self.meta_size, meta_header.meta_size)));
        }

        self.is_live = true;

        //Return SharedMem
        Ok(SharedMem {
            conf: self,
            os_data: os_map,
            user_ptr: user_ptr as *mut c_void,
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
    //User data start address
    user_ptr: *mut c_void,
}
impl<'a> SharedMem<'a> {

    ///Creates a memory mapping with no link file of specified size controlled by a single lock.
    pub fn create(lock_type: LockType, size: usize) -> Result<SharedMem<'a>> {
        SharedMemConf::new()
            .set_size(size)
            .add_lock(lock_type, 0, size).unwrap().create()
    }

    pub fn open(unique_id: &str) -> Result<SharedMem<'a>> {
        SharedMemConf::new()
            .set_os_path(unique_id)
            .open()
    }

    pub fn create_linked(new_link_path: &PathBuf, lock_type: LockType, size: usize) -> Result<SharedMem<'a>> {
        SharedMemConf::new()
            .set_link_path(new_link_path)
            .set_size(size)
            .add_lock(lock_type, 0, size).unwrap().create()
    }
    pub fn open_linked(existing_link_path: PathBuf) -> Result<SharedMem<'a>> {
        SharedMemConf::new()
            .set_link_path(&existing_link_path)
            .open()
    }

    ///Returns the size of the SharedMem
    pub fn get_size(&self) -> usize {
        self.conf.size
    }
    pub fn get_metadata_size(&self) -> usize {
        self.conf.get_metadata_size()
    }
    pub fn num_locks(&self) -> usize {
        self.conf.lock_data.len()
    }
    pub fn num_events(&self) -> usize {
        self.conf.event_data.len()
    }
    ///Returns the link_path of the SharedMem
    pub fn get_link_path(&self) -> Option<&PathBuf> {
        self.conf.link_path.as_ref()
    }
    ///Returns the OS specific path of the shared memory object
    ///
    /// Usualy on Linux, this will point to a file under /dev/shm/
    ///
    /// On Windows, this returns a namespace
    pub fn get_os_path(&self) -> &String {
        &self.os_data.unique_id
    }

    pub fn get_ptr(&self) -> *mut c_void {
        self.user_ptr
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
impl<'a> fmt::Display for SharedMem<'a> {

    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "
        Created : {}
        link : \"{}\"
        os_id : \"{}\"
        MetaSize : {}
        Size : {}
        Num locks : {}
        Num Events : {}
        MetaAddr : {:p}
        UserAddr : {:p}",
            self.conf.owner,
            self.get_link_path().unwrap_or(&PathBuf::from("[NONE]")).to_string_lossy(),
            self.get_os_path(),
            self.get_metadata_size(),
            self.get_size(),
            self.num_locks(),
            self.num_events(),
            self.os_data.map_ptr,
            self.get_ptr(),
        )
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
impl<'a> EventSet for SharedMem<'a> {
    fn set(&mut self, event_index: usize, state: EventState) -> Result<()> {
        let lock: &GenericEvent = &self.conf.event_data[event_index];
        lock.interface.set(lock.ptr, state)
    }
}
impl<'a> EventWait for SharedMem<'a> {
    fn wait(&self, event_index: usize, timeout: Timeout) -> Result<()> {
        let lock: &GenericEvent = &self.conf.event_data[event_index];
        lock.interface.wait(lock.ptr, timeout)
    }
}

///Raw shared memory mapping
///
/// This feature is only useful when dealing with memory mappings not managed by this crate.
/// When all processes involed use the shared_memory crate, it is highly recommended to avoid
/// SharedMemRaw and use the much safer/full-featured SharedMem.
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

    pub fn get_ptr(&self) -> *mut c_void {
        return self.os_data.map_ptr;
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
