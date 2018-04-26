extern crate shared_memory;
use shared_memory::*;
use std::path::PathBuf;

fn main() {

    //Open an existing raw SharedMem
    let mut my_shmem: SharedMem = match SharedMem::open_raw(String::from("some_raw_map")) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to open SharedMem...");
            return;
        }
    };

    println!("Openned link file \"{}
    Backed by OS identifier : \"{}\"
    Size : 0x{:x}",
    my_shmem.get_link_path().unwrap_or(&PathBuf::from("[NONE]")).to_string_lossy(),
    my_shmem.get_real_path().unwrap(),
    my_shmem.get_size());

    println!("Swapping first byte to 0x1 !");

    //Update the shared memory
    {
        //This uses a LockType::None which makes "locking" a no-op
        let mut num_listenners: WriteLockGuard<u8> = match my_shmem.wlock() {
            Ok(v) => v,
            Err(e) => panic!("ERROR : {}\nFailed to acquire write lock !", e),
        };
        *(*num_listenners) = 0x1;
    }

    println!("Done !");
}
