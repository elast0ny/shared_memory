//! Vecs and slices are NOT allowed in shared memory

use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub struct MyStruct {
    x: Vec<u32>,
}

#[derive(SharedMemCast)]
pub struct MyStruct2(Vec<u32>);

#[derive(SharedMemCast)]
pub struct MyStructArray<'a> {
    x: &'a [u32],
}

#[derive(SharedMemCast)]
pub struct MyStruct2Array<'a>(&'a [u32]);

fn main() {}
