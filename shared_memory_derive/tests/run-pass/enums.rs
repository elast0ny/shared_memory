use shared_memory::SharedMemCast;

#[derive(SharedMemCast)]
pub enum SimpleData {
    ItemA,
    ItemB,
    ItemC,
    ItemD,
    ItemE,
}

#[derive(SharedMemCast)]
pub enum MyData {
    Foo(i32),
    Bar {
        x: f64,
        yyy: [f64; 32],
    }
}

fn assert_impl<T: SharedMemCast>() {}
fn main() {
    assert_impl::<SimpleData>();
    assert_impl::<MyData>();
}
