//! Provides a wrapper around native shared memory for [Linux](http://man7.org/linux/man-pages/man7/shm_overview.7.html) and [Windows](http://lmgtfy.com/?q=shared+memory+windows).
//!
//! This crate is ideal if you need to share large amounts of data with another process purely through memory.
//!
//! ## Examples
//! Creator based on examples/create.rs
//! ```
//! //Create a MemFile at `pwd`\test.txt of size 4096
//! let mut mem_file: MemFile = match MemFile::create(PathBuf::from("test.txt"), 4096) {<...>};
//! //Set explicit scope for the lock (no need to call drop(shared_data))
//! {
//!     //Acquire write lock
//!     let mut shared_data = match mem_file.wlock_as_slice::<u8>() {<...>};
//!     let src = b"Some string you want to share\x00";
//!     //Write to the shared memory
//!     shared_data[0..src.len()].copy_from_slice(src);
//! }
//! ```
//!
//! Slave based on examples/open.rs
//! ```
// Open an existing MemFile from `pwd`\test.txt
//! let mut mem_file: MemFile = match MemFile::open(PathBuf::from("test.txt")) {<...>};
//! //Set explicit scope for the lock (no need to call drop(shared_data))
//! {
//!     //Acquire read lock
//!     let mut shared_data = match mem_file.rlock_as_slice::<u8>() {<...>};
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
    if #[cfg(windows)] {
        mod win;
        use win as os_impl;
    } else if #[cfg(unix)] {
        mod nix;
        use nix as os_impl;
    } else {
        compile_error!("This library isnt implemented for this platform...");
    }
}

use std::path::PathBuf;
use std::fs::{File};
use std::io::{Write, Read};
use std::fs::remove_file;
use std::os::raw::c_void;
use std::ops::{Deref, DerefMut};

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

///Struct used to manipulate the shared memory
pub struct MemFile {
    ///Meta data to help manage this MemFile
    meta: Option<os_impl::MemMetadata>,
    ///Did we create this MemFile
    owner: bool,
    ///Path to the MemFile link on disk
    link_path: Option<PathBuf>,
    ///Path to the OS's identifier for the shared memory object
    real_path: Option<String>,
    ///Size of the mapping
    size: usize,
}

impl MemFile {
    ///Opens an existing MemFile
    ///
    /// This function takes a path to a link file created by create().
    ///
    /// # Examples
    /// ```
    /// //Opens an existing shared MemFile named test.txt
    /// let mut mem_file: MemFile = match MemFile::open(PathBuf::from("test.txt")) {
    ///     Ok(v) => v,
    ///     Err(e) => {
    ///         println!("Error : {}", e);
    ///         println!("Failed to open MemFile...");
    ///         return;
    ///     }
    /// };
    /// ```
    pub fn open(existing_link_path: PathBuf) -> Result<MemFile> {

        // Make sure the link file exists
        if !existing_link_path.is_file() {
            return Err(From::from("Cannot open MemFile because file doesnt exists"));
        }

        let mut mem_file: MemFile = MemFile {
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
            mem_file.real_path = Some(String::from_utf8(file_contents)?);
        }

        //Open the shared memory using the real_path
        os_impl::open(mem_file)
    }
    ///Opens an existing shared memory object by its OS specific identifier
    pub fn open_raw(_shmem_path: String) -> Result<MemFile> {
        unimplemented!("This is not implemented yet");
    }
    /// Creates a new MemFile
    ///
    /// This involves creating a "link" on disk specified by the first parameter.
    /// This link contains the OS specific identifier to the shared memory. The usage of such link Files
    /// on disk help manage identifier colisions. (Ie: a binary using the same argument to this function
    /// can be ran from different directories without worrying about collisions)
    ///
    /// # Examples
    /// ```
    /// //Creates a new shared MemFile named test.txt of size 4096
    /// let mut mem_file: MemFile = match MemFile::create(PathBuf::from("test.txt"), 4096) {
    ///     Ok(v) => v,
    ///     Err(e) => {
    ///         println!("Error : {}", e);
    ///         println!("Failed to create MemFile...");
    ///         return;
    ///     }
    /// };
    /// ```
    pub fn create(new_link_path: PathBuf, size: usize) -> Result<MemFile> {

        let mut cur_link;
        if new_link_path.is_file() {
            return Err(From::from("Cannot create MemFile because file already exists"));
        } else {
            cur_link = File::create(&new_link_path)?;
        }

        let mem_file: MemFile = MemFile {
            meta: None,
            owner: true,
            link_path: Some(new_link_path),
            real_path: None,
            size: size,
        };

        let created_file = os_impl::create(mem_file)?;

        //Write OS specific identifier in link file
        {
            let real_path: &String = created_file.real_path.as_ref().unwrap();
            match cur_link.write(real_path.as_bytes()) {
                Ok(write_sz) => if write_sz != real_path.as_bytes().len() {
                    return Err(From::from("Failed to write full contents info on disk"));
                },
                Err(_) => return Err(From::from("Failed to write info on disk")),
            };
        }

        Ok(created_file)
    }
    ///Creates a shared memory object
    pub fn create_raw(_shmem_path: String) -> Result<MemFile> {
        unimplemented!("This is not implemented yet");
    }
    ///Returns the size of the MemFile
    pub fn get_size(&self) -> &usize {
        &self.size
    }
    ///Returns the link_path of the MemFile
    pub fn get_link_path(&self) -> Option<&PathBuf> {
        self.link_path.as_ref()
    }
    ///Returns the OS specific path of the shared memory object
    ///
    /// Usualy on Linux, this will point to a file under /dev/shmem
    ///
    /// On Windows, this returns a namespace
    pub fn get_real_path(&self) -> Option<&String> {
        self.real_path.as_ref()
    }
    ///Returns a non-exclusive read lock to the shared memory
    ///
    /// # Examples
    ///
    /// ```
    /// //let some_val: MemFileRLock<u8> = mem_file.rlock().unwrap();
    /// let some_val = mem_file.rlock::<u8>().unwrap();
    /// println!("I can read a shared u8 ! {}", *some_val);
    /// ```
    pub fn rlock<T: MemFileCast>(&self) -> Result<MemFileRLock<T>> {

        //Make sure we have a file mapped
        if let Some(ref meta) = self.meta {

            //Make sure that we can cast our memory to the type
            let type_size = std::mem::size_of::<T>();
            if type_size > self.size {
                return Err(From::from(
                    format!("Tried to map MemFile to a too big type {}/{}", type_size, self.size)
                ));
            }
            return Ok(os_impl::rlock::<T>(meta));
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }
    ///Returns a non-exclusive read lock to the shared memory as a slice
    ///
    /// # Examples
    ///
    /// ```
    /// //let read_buf: MemFileRLockSlice<u8> = mem_file.rlock_as_slice().unwrap();
    /// let read_buf = mem_file.rlock_as_slice::<u8>().unwrap();
    /// println!("I'm reading into a u8 from a shared &[u8] ! : {}", read_buf[0]);
    /// ```
    pub fn rlock_as_slice<T: MemFileCast>(&self) -> Result<MemFileRLockSlice<T>> {

        //Make sure we have a file mapped
        if let Some(ref meta) = self.meta {

            //Make sure that we can cast our memory to the slice
            let item_size = std::mem::size_of::<T>();
            if item_size > self.size {
                return Err(From::from(
                    format!("Tried to map MemFile to a too big type {}/{}", item_size, self.size)
                ));
            }
            let num_items: usize = self.size / item_size;

            return Ok(os_impl::rlock_slice::<T>(meta, 0, num_items));
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }
    ///Returns an exclusive read/write lock to the shared memory
    /// # Examples
    ///
    /// ```
    /// //let mut some_val: MemFileWLock<u32> = mem_file.wlock().unwrap();
    /// let mut some_val = mem_file.wlock::<u32>().unwrap();
    /// *(*some_val) = 1;
    /// ```
    pub fn wlock<T: MemFileCast>(&mut self) -> Result<MemFileWLock<T>> {

        //Make sure we have a file mapped
        if let Some(ref mut meta) = self.meta {

            //Make sure that we can cast our memory to the type
            let type_size = std::mem::size_of::<T>();
            if type_size > self.size {
                return Err(From::from(
                    format!("Tried to map MemFile to a too big type {}/{}", type_size, self.size)
                ));
            }

            return Ok(os_impl::wlock::<T>(meta));
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }
    ///Returns exclusive read/write access to a &mut [T] on the shared memory
    ///
    /// # Examples
    ///
    /// ```
    /// //let write_buf: MemFileWLockSlice<u8> = mem_file.wlock_as_slice().unwrap();
    /// let write_buf = mem_file.wlock_as_slice::<u8>().unwrap();
    /// write_buf[0] = 0x1;
    /// ```
    pub fn wlock_as_slice<T: MemFileCast>(&mut self) -> Result<MemFileWLockSlice<T>> {

        //Make sure we have a file mapped
        if let Some(ref mut meta) = self.meta {

            //Make sure that we can cast our memory to the slice
            let item_size = std::mem::size_of::<T>();
            if item_size > self.size {
                return Err(From::from(
                    format!("Tried to map MemFile to a too big type {}/{}", item_size, self.size)
                ));
            }
            let num_items: usize = self.size / item_size;

            return Ok(os_impl::wlock_slice::<T>(meta, 0, num_items));
        } else {
            return Err(From::from("No file mapped to get lock on"));
        }
    }
}

impl Drop for MemFile {
    ///Deletes the MemFile artifacts
    fn drop(&mut self) {
        //Delete file on disk if we created it
        if self.owner {
            if let Some(ref file_path) = self.link_path {
                if file_path.is_file() {
                    match remove_file(file_path) {_=>{},};
                }
            }
        }
        //Drop our internal view of the MemFile
        if let Some(meta) = self.meta.take() {
            drop(meta);
        }
    }
}

/// Read [WARNING](trait.MemFileCast.html#warning) before use
///
/// Trait used to indicate that a type can be cast over the shared memory.
///
/// For now, mem_file implements the trait on almost all primitive types.
///
/// ### __<span style="color:red">WARNING</span>__
///
/// Only implement this trait if you understand the implications of mapping Rust types to shared memory.
/// When doing so, you should be mindful of :
/// * Does my type have any pointers in its internal representation ?
///    * This is important because pointers in your type need to also point to the shared memory for it to be usable by other processes
/// * Can my type resize its contents ?
///    * If so, the type probably cannot be safely used over shared memory because your type might call Alloc/Free on a shared memory addresses
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
/// impl MemFileCast for SharedState {}
///
/// <...>
///
/// {
///     let mut shared_state: MemFileWLock<SharedState> = match mem_file.wlock() {
///         Ok(v) => v,
///         Err(_) => panic!("Failed to acquire write lock !"),
///     };
///
///     shared_state.num_listenners = 0;
/// }
///```
pub unsafe trait MemFileCast {}
unsafe impl MemFileCast for bool {}
unsafe impl MemFileCast for char {}
unsafe impl MemFileCast for str {}
unsafe impl MemFileCast for i8 {}
unsafe impl MemFileCast for i16 {}
unsafe impl MemFileCast for i32 {}
unsafe impl MemFileCast for u8 {}
unsafe impl MemFileCast for i64 {}
unsafe impl MemFileCast for u16 {}
unsafe impl MemFileCast for u64 {}
unsafe impl MemFileCast for isize {}
unsafe impl MemFileCast for u32 {}
unsafe impl MemFileCast for usize {}
unsafe impl MemFileCast for f32 {}
unsafe impl MemFileCast for f64 {}

/* Read Locks */

/// Non-exclusive read lock holding a reference to shared memory
///
/// To get an instance of this struct, see [rlock()](struct.MemFile.html#method.rlock)
pub struct MemFileRLock<'a, T: 'a> {
    data: &'a T,
    lock: *mut c_void,
}
impl<'a, T> Drop for MemFileRLock<'a, T> { fn drop(&mut self) { os_impl::read_unlock(self.lock); } }
impl<'a, T> Deref for MemFileRLock<'a, T> {
    type Target = &'a T;
    fn deref(&self) -> &Self::Target { &self.data }
}

/// Non-exclusive read lock holding a reference to a slice of shared memory
///
/// To get an instance of this struct, see [rlock_as_slice()](struct.MemFile.html#method.rlock_as_slice)
pub struct MemFileRLockSlice<'a, T: 'a> {
    data: &'a [T],
    lock: *mut c_void,
}
impl<'a, T> Drop for MemFileRLockSlice<'a, T> { fn drop(&mut self) { os_impl::read_unlock(self.lock); } }
impl<'a, T> Deref for MemFileRLockSlice<'a, T> {
    type Target = &'a [T];
    fn deref(&self) -> &Self::Target { &self.data }
}

/* Write Locks */

/// Exclusive write lock holding a reference to shared memory
///
/// To get an instance of this struct, see [wlock()](struct.MemFile.html#method.wlock)
pub struct MemFileWLock<'a, T: 'a> {
    data: &'a mut T,
    lock: *mut c_void,
}
impl<'a, T> Drop for MemFileWLock<'a, T> { fn drop(&mut self) { os_impl::write_unlock(self.lock); } }
impl<'a, T> Deref for MemFileWLock<'a, T> {
    type Target = &'a mut T;
    fn deref(&self) -> &Self::Target { &self.data }
}
impl<'a, T> DerefMut for MemFileWLock<'a, T> {
    fn deref_mut(&mut self) -> &mut &'a mut T {
        &mut self.data
    }
}

/// Exclusive write lock holding a reference to a slice of shared memory
///
/// To get an instance of this struct, see [wlock_as_slice()](struct.MemFile.html#method.wlock_as_slice)
pub struct MemFileWLockSlice<'a, T: 'a> {
    data: &'a mut [T],
    lock: *mut c_void,
}
impl<'a, T> Drop for MemFileWLockSlice<'a, T> { fn drop(&mut self) { os_impl::write_unlock(self.lock); } }
impl<'a, T> Deref for MemFileWLockSlice<'a, T> {
    type Target = &'a mut [T];
    fn deref(&self) -> &Self::Target { &self.data }
}
impl<'a, T> DerefMut for MemFileWLockSlice<'a, T> {
    fn deref_mut(&mut self) -> &mut &'a mut [T] {
        &mut self.data
    }
}
