extern crate shared_memory;
use shared_memory::*;
use std::path::PathBuf;

//This example demonstrates how to use the *_raw() APIs.
//
//These APIs are only useful if you wish to use shared memory that isnt managed by my_shmem.

fn main() {

    //Create a new raw shared mapping
    let my_shmem: SharedMem = match SharedMem::create_raw(String::from("some_raw_map"), 4096) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to create raw SharedMem...");
            return;
        }
    };

    //Display some info
    println!("Created link file \"{}\"
    Backed by OS identifier : \"{}\"
    Size : 0x{:x}",
    my_shmem.get_link_path().unwrap_or(&PathBuf::from("[NONE]")).to_string_lossy(),
    my_shmem.get_real_path().unwrap(),
    my_shmem.get_size());

    println!("Busy looping until first byte changes...");
    {
        //This uses a LockType::None which makes "locking" a no-op
        let first_byte: ReadLockGuard<u8> = my_shmem.rlock().unwrap();

        //We never need to release the "lock" since there is no lock
        while *first_byte == &0 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    println!("Done !");
}
