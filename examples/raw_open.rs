extern crate shared_memory;
use shared_memory::{
    SharedMemRaw,
    WriteRaw,
};
use std::sync::atomic::*;

fn main() {

    //Open an existing raw SharedMem
    let mut my_shmem: SharedMemRaw = match SharedMemRaw::open(String::from("some_raw_map")) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to open SharedMem...");
            return;
        }
    };

    println!("Openned raw map @ \"{}\"
    Size : 0x{:x}",
    my_shmem.get_path(),
    my_shmem.get_size());

    println!("Swapping first byte to 0x1 !");

    //Update the shared memory
    let first_byte: &mut AtomicBool = unsafe { my_shmem.get_raw_mut() };
    first_byte.store(true, Ordering::Relaxed);

    println!("Done !");
}
