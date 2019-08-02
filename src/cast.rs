use std::sync::atomic::*;

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

unsafe impl<T: SharedMemCast> SharedMemCast for [T] {}

macro_rules! array_impl {
    ($($n:expr),*) => {
        $(
            unsafe impl<T: SharedMemCast> SharedMemCast for [T; $n] {}
        )*
    };
}

// Implementations for [T; 1] to [T; 32].
// Followed by powers of 2 up to 2^31 (since [u8; 2^31] is ~2 GB which seems like more than enough)
array_impl!(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
    23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384,
    32768, 65536, 131072, 262144, 524288, 1048576, 2097152, 4194304, 8388608, 16777216, 33554432,
    67108864, 134217728, 268435456, 536870912, 1073741824, 2147483648);
