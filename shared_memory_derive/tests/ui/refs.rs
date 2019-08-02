//! References and trait objects are NOT allowed in shared memory

use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub struct MyStruct<'a> {
    x: &'a i32,
}

#[derive(SharedMemCast)]
pub struct MyStruct2<'a>(&'a i32);

#[derive(SharedMemCast)]
pub struct MyStructTraitObject<'a> {
    x: &'a dyn SharedMemCast,
}

#[derive(SharedMemCast)]
pub struct MyStruct2TraitObject<'a>(&'a dyn SharedMemCast);

// No lifetime on struct itself so forced to reject based on field
#[derive(SharedMemCast)]
pub struct MyStructStatic<T: SharedMemCast + 'static> {
    x: &'static T,
}

#[derive(SharedMemCast)]
pub struct MyStruct2Static<T: SharedMemCast + 'static>(&'static T);

fn main() {}
