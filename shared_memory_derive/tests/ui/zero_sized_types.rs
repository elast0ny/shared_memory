//! Zero sized types are NOT allowed

use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub enum EmptyEnum {}

//TODO: Empty structs may actually be okay?
#[derive(SharedMemCast)]
pub struct Empty;

#[derive(SharedMemCast)]
pub struct Empty2 {
}

//TODO: Empty fields may actually be okay?
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
