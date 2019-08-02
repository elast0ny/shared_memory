//! We should assert that each parameter type implements the trait and fail to
//! compile if it does not.

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

pub struct NotShared;

fn assert_impl<T: SharedMemCast>() {}
fn main() {
    assert_impl::<MyStruct<NotShared>>();
    assert_impl::<MyStruct<Vec<i32>>>();
    assert_impl::<MyStruct<&[i32]>>();
    fn generic_param<U>() {
        assert_impl::<MyStruct<U>>();
    }

    assert_impl::<MyStruct2<NotShared>>();
    assert_impl::<MyStruct2<Vec<i32>>>();
    assert_impl::<MyStruct2<&[i32]>>();
    fn generic_param2<U>() {
        assert_impl::<MyStruct2<U>>();
    }

    assert_impl::<MyData<NotShared>>();
    assert_impl::<MyData<Vec<i32>>>();
    assert_impl::<MyData<&[i32]>>();
    fn generic_param3<U>() {
        assert_impl::<MyData<U>>();
    }
}
