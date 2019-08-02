//! Generics must have a SharedMemCast bound.
//!
//! No bound on T = no guarantee for SharedMemCast

use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub struct MyStruct<T> {
    x: T,
}

#[derive(SharedMemCast)]
pub struct MyStruct2<T>(T);

#[derive(SharedMemCast)]
pub enum MyData<T> {
    Foo(i32),
    Bar {
        x: f64,
        yyy: [f64; 32],
        val: T,
    }
}

fn main() {}
