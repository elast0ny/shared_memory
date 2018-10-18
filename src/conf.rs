extern crate rand;
use self::rand::Rng;
extern crate theban_interval_tree;
use self::theban_interval_tree::*;
extern crate memrange;
use self::memrange::Range;

use super::*;
use enum_primitive::FromPrimitive;

use std::io::{Write, Read};
use std::ptr::null_mut;
use std::mem::size_of;

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
#[repr(C)]
struct MetaDataHeader {
    meta_size: u64,
    user_size: u64,
    num_locks: u64,
    num_events: u64,
}
#[repr(C)]
struct LockHeader {
    uid: u8,
    offset: u64,
    length: u64,
}
#[repr(C)]
struct EventHeader {
    uid: u8,
}

///Configuration used to describe a shared memory mapping before openning/creation
pub struct SharedMemConf<'a> {
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
    //Adds a lock to our config
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
    //Adds an event to our config
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
    //Calculates the meta data size required given the current config
    fn calculate_metadata_size(&self) -> usize {

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

    ///Returns a new SharedMemConf
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
    ///Adds a lock of specified type on a range of bytes
    pub fn add_lock(mut self, lock_type: LockType, offset: usize, length: usize) -> Result<SharedMemConf<'a>> {
        self.add_lock_impl(lock_type, offset, length)?;
        Ok(self)
    }
    ///Adds an event of specified type
    pub fn add_event(mut self, event_type: EventType) -> Result<SharedMemConf<'a>> {
        self.add_event_impl(event_type)?;
        Ok(self)
    }
    ///Creates a shared memory mapping from the current config values
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

        let meta_size: usize = self.calculate_metadata_size();
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
        meta_header.meta_size = meta_size as u64;
        meta_header.user_size = self.size as u64;
        meta_header.num_locks = self.lock_data.len() as u64;
        meta_header.num_events = self.event_data.len() as u64;
        cur_ptr += size_of::<MetaDataHeader>();

        //Initialize locks
        for lock in &mut self.lock_data {
            //Set lock header
            let lock_header: &mut LockHeader = unsafe{&mut (*(cur_ptr as *mut LockHeader))};
            lock_header.uid = lock.uid;
            lock_header.offset = lock.offset as u64;
            lock_header.length = lock.length as u64;
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

        Ok(SharedMem {
            conf: self,
            os_data: os_map,
            user_ptr: cur_ptr as *mut c_void,
            link_file: cur_link,
        })
    }
    ///Opens a shared memory mapping.
    ///
    ///This will look at the current link_path/os_path to create the SharedMem. Other values will be reset.
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

        //Reset config fields in case user modifed them before open()
        self.lock_range_tree = IntervalTree::<usize>::new();
        self.lock_data = Vec::with_capacity(2);
        self.event_data = Vec::with_capacity(2);

        if size_of::<MetaDataHeader>() > os_map.map_size {
            return Err(From::from("Mapping is smaller than our metadata header size !"));
        }

        //Initialize meta data
        let mut cur_ptr = os_map.map_ptr as usize;

        //Read header for basic info
        let meta_header: &mut MetaDataHeader = unsafe{&mut (*(cur_ptr as *mut MetaDataHeader))};
        cur_ptr += size_of::<MetaDataHeader>();

        self.size = meta_header.user_size as usize;

        //Basic size check on (metadata size + userdata size)
        if (os_map.map_size as u64) < (meta_header.meta_size + meta_header.user_size) {
            return Err(From::from(
                format!("Shared memory header contains an invalid mapping size : (map_size: {}, meta_size: {}, user_size: {})",
                    os_map.map_size,
                    meta_header.user_size,
                    meta_header.meta_size)
            ));
        }

        //Add the metadata size to our base pointer to get user addr
        let user_ptr = os_map.map_ptr as usize + meta_header.meta_size as usize;

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
            self.add_lock_impl(lock_type, lock_header.offset as usize, lock_header.length as usize)?;

            let new_lock: &mut GenericLock = self.lock_data.last_mut().unwrap();

            new_lock.lock_ptr = cur_ptr as *mut c_void;
            new_lock.data_ptr = (user_ptr + lock_header.offset as usize) as *mut c_void;

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
        } else if self.meta_size as u64 != meta_header.meta_size {
            return Err(From::from(format!("Shared memory metadata does not match what was advertised ! {} != {}", self.meta_size, meta_header.meta_size)));
        }

        //Return SharedMem
        Ok(SharedMem {
            conf: self,
            os_data: os_map,
            user_ptr: user_ptr as *mut c_void,
            link_file: cur_link,
        })
    }

    /* Helper function for read only access to private members */

    #[inline]
    ///Returns the currently set link_path value
    pub fn get_link_path(&self) -> Option<&PathBuf> {
        self.link_path.as_ref()
    }
    #[inline]
    ///Returns the currently set os_path value
    pub fn get_os_path(&self) -> Option<&String> {
        self.wanted_os_path.as_ref()
    }
    #[inline]
    ///Return the current size of the user data
    pub fn get_size(&self) -> usize {
        self.size
    }
    #[inline]
    ///Returns the current size that the metadata will take
    pub fn get_metadata_size(&self) -> usize {
        self.meta_size
    }
    #[inline]
    ///Returns the current number of locks
    pub fn num_locks(&self) -> usize {
        self.lock_data.len()
    }

    #[inline]
    ///Returns the current number of events
    pub fn num_events(&self) -> usize {
        self.event_data.len()
    }

    #[doc(hidden)]
    #[inline]
    pub fn is_owner(&self) -> bool {
        self.owner
    }

    #[doc(hidden)]
    #[inline]
    pub fn get_lock(&self, lock_index: usize) -> &GenericLock {
        &self.lock_data[lock_index]
    }

    #[doc(hidden)]
    #[inline]
    pub fn get_event(&self, event_index: usize) -> &GenericEvent {
        &self.event_data[event_index]
    }
}
