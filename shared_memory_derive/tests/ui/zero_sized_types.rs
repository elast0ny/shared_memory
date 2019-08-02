//! Zero sized types are NOT allowed

use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub enum EmptyEnum {}

#[derive(SharedMemCast)]
pub struct Empty;

#[derive(SharedMemCast)]
pub struct Empty2 {
}

#[derive(SharedMemCast)]
pub struct Empty3();

#[derive(SharedMemCast)]
pub struct StillEmpty {
    x: (),
}

#[derive(SharedMemCast)]
pub struct FieldEmpty {
    y: i32,
    x: (),
    z: f64,
    w: [i32; 0],
}

fn main() {}
