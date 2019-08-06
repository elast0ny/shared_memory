use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub struct MyStruct(u8);

#[derive(SharedMemCast)]
pub struct MyStruct2(u8, f64, i32, usize);

fn assert_impl<T: SharedMemCast>() {}
fn main() {
    assert_impl::<MyStruct>();
    assert_impl::<MyStruct2>();
}
