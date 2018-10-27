extern crate shared_memory;
use shared_memory::{
    SharedMemRaw,
    ReadRaw,
};
use std::sync::atomic::*;

//This example demonstrates how one can create a raw memory mapping with no bells and whistles

fn main() {

    //Create a new raw shared mapping
    let my_shmem: SharedMemRaw = match SharedMemRaw::create("some_raw_map", 4096) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to create raw SharedMem...");
            return;
        }
    };

    //Display some info
    println!("Created raw map @ \"{}\"
    Size : 0x{:x}",
    my_shmem.get_path(),
    my_shmem.get_size());

    println!("Busy looping until first byte changes...");

    //On most architectures, reading a byte is always atomic but oh well
    let first_byte: &AtomicBool = unsafe { my_shmem.get_raw() };

    while !first_byte.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    println!("Done !");
}
