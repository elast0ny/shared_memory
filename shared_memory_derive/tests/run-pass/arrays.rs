use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub struct MyStruct {
    x: [u8; 1],
}

#[derive(SharedMemCast)]
pub struct MyStruct2 {
    x: [u8; 2],
    y: [f64; 32],
    z: i32,
    w: [usize; 128],
}

fn assert_impl<T: SharedMemCast>() {}
fn main() {
    assert_impl::<MyStruct>();
    assert_impl::<MyStruct2>();
}
