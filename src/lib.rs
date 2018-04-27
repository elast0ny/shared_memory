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
    } else if #[cfg(target_os="linux")] {
        mod linux;
        use linux as os_impl;
    } else if #[cfg(target_os="macos")] {
        mod macos;
        use macos as os_impl;
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

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

///Struct used to manipulate the shared memory
pub struct SharedMem<'a> {
    ///Meta data to help manage this SharedMem
    meta: Option<os_impl::MemMetadata<'a>>,
    ///Did we create this SharedMem
    owner: bool,
    ///Path to the SharedMem link on disk
    link_path: Option<PathBuf>,
    ///Path to the OS's identifier for the shared memory object
    real_path: Option<String>,
    ///Size of the mapping
    size: usize,
}

impl<'a> SharedMem<'a> {
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
            return Err(From::from("Cannot open SharedMem because file doesnt exists"));
        }

        let mut my_shmem: SharedMem = SharedMem {
            meta: None,
            owner: false,
            link_path: Some(existing_link_path.clone()),
            real_path: None,
            size: 0, //os_open needs to fill this field up
        };

        //Get real_path from link file
        {
            let mut disk_file = File::open(&existing_link_path)?;
            let mut file_contents: Vec<u8> = Vec::with_capacity(existing_link_path.to_string_lossy().len() + 5);
            disk_file.read_to_end(&mut file_contents)?;
            my_shmem.real_path = Some(String::from_utf8(file_contents)?);
        }

        //Open the shared memory using the real_path
        os_impl::open(my_shmem)
    }
    ///Creates a raw shared memory object. Only use this method if you do not wish to have all the nice features of a regular SharedMem.
    ///
    ///This function is useful when creating mappings for libraries/applications that do not use SharedMem.
    ///By using this function, you explicitly mean : do not create anything else than a memory mapping.
    ///
    ///The first argument needs to be a valid identifier for the OS in use.
    ///colisions wont be avoided through link files and no meta data (locks) is added to the shared mapping.
    pub fn create_raw(shmem_path: String, size: usize) -> Result<SharedMem<'a>> {
        let my_shmem: SharedMem = SharedMem {
            meta: None,
            owner: true,
            link_path: None, //Leave this explicitly empty
            real_path: Some(shmem_path),
            size: size,
        };

        Ok(os_impl::create(my_shmem, LockType::None)?)
    }
    ///Opens an existing shared memory mappping in raw mode.
    ///This simply opens an existing mapping with no additionnal features (no locking, no metadata, etc...).
    ///
    ///This function is useful when using mappings not created by my_shmem.
    ///
    ///To use this function, you need to pass a valid OS shared memory identifier as an argument.
    pub fn open_raw(shmem_path: String) -> Result<SharedMem<'a>> {

        let my_shmem: SharedMem = SharedMem {
            meta: None,
            owner: false,
            link_path: None, //Leave this explicity to None to specify raw mode
            real_path: Some(shmem_path),
            size: 0, //os_open needs to fill this field up
        };

        //Open the shared memory using the real_path
        os_impl::open(my_shmem)
    }

    ///Returns the size of the SharedMem
    pub fn get_size(&self) -> &usize {
        &self.size
    }
    ///Returns the link_path of the SharedMem
    pub fn get_link_path(&self) -> Option<&PathBuf> {
        self.link_path.as_ref()
    }
    ///Returns the OS specific path of the shared memory object
    ///
    /// Usualy on Linux, this will point to a file under /dev/shm/
    ///
    /// On Windows, this returns a namespace
    pub fn get_real_path(&self) -> Option<&String> {
        self.real_path.as_ref()
    }
}

impl<'a> Drop for SharedMem<'a> {
    ///Deletes the SharedMem artifacts
    fn drop(&mut self) {
        //Delete file on disk if we created it
        if self.owner {
            if let Some(ref file_path) = self.link_path {
                if file_path.is_file() {
                    match remove_file(file_path) {_=>{},};
                }
            }
        }
        //Drop our internal view of the SharedMem
        if let Some(meta) = self.meta.take() {
            drop(meta);
        }
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
