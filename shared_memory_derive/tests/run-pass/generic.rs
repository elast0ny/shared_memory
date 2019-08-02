//! It is typical for macros to insert the T: SharedMemCast bound automatically

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

fn assert_impl<T: SharedMemCast>() {}
fn main() {
    assert_impl::<MyStruct<i32>>();
    assert_impl::<MyStruct<f64>>();
    assert_impl::<MyStruct<[u32; 8]>>();
    fn generic_param<U: SharedMemCast>() {
        assert_impl::<MyStruct<U>>();
    }

    assert_impl::<MyStruct2<i32>>();
    assert_impl::<MyStruct2<f64>>();
    assert_impl::<MyStruct2<[u32; 8]>>();
    fn generic_param2<U: SharedMemCast>() {
        assert_impl::<MyStruct2<U>>();
    }

    assert_impl::<MyData<i32>>();
    assert_impl::<MyData<f64>>();
    assert_impl::<MyData<[u32; 8]>>();
    fn generic_param3<U: SharedMemCast>() {
        assert_impl::<MyData<U>>();
    }
}
