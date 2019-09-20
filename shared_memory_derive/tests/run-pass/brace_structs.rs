use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub struct MyStruct {
    x: u8,
}

#[derive(SharedMemCast)]
pub struct MyStruct2 {
    x: u8,
    y: f64,
    z: i32,
    w: usize,
}

fn assert_impl<T: SharedMemCast>() {}
fn main() {
    assert_impl::<MyStruct>();
    assert_impl::<MyStruct2>();
}
