use std::mem;
use std::sync::Arc;

use serde::{Serialize, Deserialize};
use serde::de::Deserializer;
use serde::ser::Serializer;

use crate::{
    LockType, ReadLockGuard, ReadLockable, SharedMem, SharedMemCast, WriteLockGuard, WriteLockable, SharedMemError,
};

/// A handle lets you share objects across processes with serde.
///
/// This abstracts over shared memory in a way that an object can be serialized
/// across process boundaries.  This lets you take a `SharedMemCast` object
/// and serialize it with serde so it can be used from two processes.
///
/// This is useful in combination with crates like `procspawn`.
pub struct Handle<T> {
    mem: Arc<SharedMem>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Handle<T> {
        Handle {
            mem: self.mem.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: SharedMemCast> Handle<T> {
    /// Creates a new handle wrapping a shared object.
    ///
    // This abstracts over shared memory in a way that an object can be serialized
    // across process boundaries.  This lets you take a `SharedMemCast` object
    // and serialize it with serde so it can be used from two processes.
    /// This is useful in combination with crates like `procspawn`.
    ///
    /// This object is locked with a mutex.
    pub fn new(value: T) -> Result<Handle<T>, SharedMemError> {
        Handle::with_lock(LockType::Mutex, value)
    }

    /// Creates a new handle wrapping a shared object.
    ///
    /// This handle is locked with the given lock type.
    pub fn with_lock(lock: LockType, value: T) -> Result<Handle<T>, SharedMemError> {
        let mem = SharedMem::create(lock, mem::size_of::<T>())?;
        {
            let mut data: WriteLockGuard<T> = mem.wlock(0).unwrap();
            mem::replace(&mut **data, value);
        }
        Ok(Handle {
            mem: Arc::new(mem),
            _marker: std::marker::PhantomData,
        })
    }

    /// Acquires a write lock.
    pub fn wlock(&self) -> Result<WriteLockGuard<T>, SharedMemError> {
        self.mem.wlock(0)
    }

    /// Acquires a read lock.
    pub fn rlock(&self) -> Result<ReadLockGuard<T>, SharedMemError> {
        self.mem.rlock(0)
    }
}

impl<T: SharedMemCast> Serialize for Handle<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.mem.get_os_path())
    }
}

impl<'de, T: SharedMemCast> Deserialize<'de> for Handle<T> {
    fn deserialize<D>(deserializer: D) -> Result<Handle<T>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = String::deserialize(deserializer)?;
        Ok(Handle {
            mem: Arc::new(SharedMem::open(&s).unwrap()),
            _marker: std::marker::PhantomData,
        })
    }
}
