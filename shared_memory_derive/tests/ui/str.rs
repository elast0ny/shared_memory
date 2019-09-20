//! Since only `str` implements SharedMemCast, it should be impossible to store a `&str` in
//! a struct and then derive SharedMemCast. This would be invalid anyway since you can't store
//! references in shared memory.

use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub struct MyStruct<'a> {
    x: &'a str,
}

#[derive(SharedMemCast)]
pub struct MyStruct2<'a>(&'a str);

// No lifetime on struct itself so forced to reject based on field
#[derive(SharedMemCast)]
pub struct MyStructStatic {
    x: &'static str,
}

#[derive(SharedMemCast)]
pub struct MyStruct2Static(&'static str);

fn main() {}
