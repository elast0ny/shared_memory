use super::*;
use std::ops::{Deref, DerefMut};
use std::slice;

pub enum LockType {
    Mutex,
    Rwlock,
    None,
}

#[doc(hidden)] pub struct LockNone {}
#[doc(hidden)] impl MemFileLockable for LockNone {
    fn rlock(&self) -> Result<()> {
        println!("Read lock acquired !");
        Ok(())
    }
    fn wlock(&self) -> Result<()> {
        println!("Write lock acquired !");
        Ok(())
    }
    fn runlock(&self) -> () {
        println!("Read lock released !");
    }
    fn wunlock(&self) -> () {
        println!("Write lock released !");
    }
}

///Plublic traits that custom locks need to implement
#[doc(hidden)] pub trait MemFileLockable {
    ///This method should only return once we have safe read access
    fn rlock(&self) -> Result<()>;
    ///This method should only return once we have safe write access
    fn wlock(&self) -> Result<()>;

    ///This method is automatically called when a read lock guards is dropped
    fn runlock(&self) -> ();
    ///This method is automatically called when a read lock guards is dropped
    fn wunlock(&self) -> ();
}


#[doc(hidden)] impl<'a> os_impl::MemMetadata<'a> {
    pub fn rlock<'b, B: MemFileCast>(&'b self) -> ReadLockGuard<'b, B> {
        //Call the custom lock impl
        self.lock.rlock().unwrap();

        //Return data wrapped in a lock
        unsafe {
            ReadLockGuard {
                data: &(*(self.data as *const B)),
                //Set the custom unlock trait
                lock: self.lock,
            }
        }
    }

    pub fn rlock_as_slice<'b, B: MemFileCast>(&'b self, start_offset: usize, num_elements:usize) -> ReadLockGuardSlice<'b, B> {
        //Call the custom lock impl
        self.lock.rlock().unwrap();

        //Return data wrapped in a lock
        unsafe {
            ReadLockGuardSlice {
                data: slice::from_raw_parts((self.data as usize + start_offset) as *const B, num_elements),
                //Set the custom unlock trait
                lock: self.lock,
            }
        }
    }

    pub fn wlock<'b, B: MemFileCast>(&'b self) -> WriteLockGuard<'b, B> {
        //Call the custom lock impl
        self.lock.wlock().unwrap();

        //Return data wrapped in a lock
        unsafe {
            WriteLockGuard {
                data: &mut (*(self.data as *mut B)),
                //Set the custom unlock trait
                lock: self.lock,
            }
        }
    }

    pub fn wlock_as_slice<'b, B: MemFileCast>(&'b self, start_offset: usize, num_elements:usize) -> WriteLockGuardSlice<'b, B> {
        //Call the custom lock impl
        self.lock.wlock().unwrap();

        //Return data wrapped in a lock
        unsafe {
            WriteLockGuardSlice {
                data: slice::from_raw_parts_mut((self.data as usize + start_offset) as *mut B, num_elements),
                //Set the custom unlock trait
                lock: self.lock,
            }
        }
    }
}


/* Lock Guards */

//Read
pub struct ReadLockGuard<'a, T: 'a> {
    data: &'a T,
    lock: &'a MemFileLockable,
}
impl<'a, T: 'a> Drop for ReadLockGuard<'a, T> {
    fn drop(&mut self) -> () {
        self.lock.runlock();
    }
}
impl<'a, T> Deref for ReadLockGuard<'a, T> {
    type Target = &'a T;
    fn deref(&self) -> &Self::Target { &self.data }
}
//Read Slice
pub struct ReadLockGuardSlice<'a, T: 'a> {
    data: &'a [T],
    lock: &'a MemFileLockable,
}
impl<'a, T: 'a> Drop for ReadLockGuardSlice<'a, T> {
    fn drop(&mut self) -> () {
        self.lock.runlock();
    }
}
impl<'a, T> Deref for ReadLockGuardSlice<'a, T> {
    type Target = &'a [T];
    fn deref(&self) -> &Self::Target { &self.data }
}

//Write
pub struct WriteLockGuard<'a, T: 'a> {
    data: &'a mut T,
    lock: &'a MemFileLockable,
}
impl<'a, T: 'a> Drop for WriteLockGuard<'a, T> {
    fn drop(&mut self) -> () {
        self.lock.wunlock();
    }
}
impl<'a, T> Deref for WriteLockGuard<'a, T> {
    type Target = &'a mut T;
    fn deref(&self) -> &Self::Target { &self.data }
}
impl<'a, T> DerefMut for WriteLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut &'a mut T {
        &mut self.data
    }
}

//Write Slice
pub struct WriteLockGuardSlice<'a, T: 'a> {
    data: &'a mut [T],
    lock: &'a MemFileLockable,
}
impl<'a, T: 'a> Drop for WriteLockGuardSlice<'a, T> {
    fn drop(&mut self) -> () {
        self.lock.wunlock();
    }
}
impl<'a, T> Deref for WriteLockGuardSlice<'a, T> {
    type Target = &'a mut [T];
    fn deref(&self) -> &Self::Target { &self.data }
}
impl<'a, T> DerefMut for WriteLockGuardSlice<'a, T> {
    fn deref_mut(&mut self) -> &mut &'a mut [T] {
        &mut self.data
    }
}
