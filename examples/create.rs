extern crate shared_memory;
use shared_memory::*;
use std::path::PathBuf;

struct SharedState {
    num_listenners: u32,
    message: [u8; 256],
}
//WARNING : Only do this if you know what you're doing.
unsafe impl SharedMemCast for SharedState {}

fn main() {

    let lock_type = LockType::Mutex;

    //Create a new shared SharedMem
    let mut my_shmem: SharedMem = match SharedMem::create(PathBuf::from("shared_mem.link"),  lock_type, 4096) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to create SharedMem...");
            return;
        }
    };

    println!("Created link file \"{}\"
    Backed by OS identifier : \"{}\"
    Size : 0x{:x}",
    my_shmem.get_link_path().unwrap().to_string_lossy(),
    my_shmem.get_real_path().unwrap(),
    my_shmem.get_size());

    //Initialize the memory with default values
    {
        let mut shared_state: WriteLockGuard<SharedState> = match my_shmem.wlock() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        shared_state.num_listenners = 0;
        let src = b"Welcome, we currently have 0 listenners !\x00";
        shared_state.message[0..src.len()].copy_from_slice(src);

        println!("Holding lock for 5 seconds !");
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
    println!("Waiting for a listenner to connect !");

    //Loop until our memory has changed
    loop {

        //Acquire read lock
        let shared_state: ReadLockGuard<SharedState> = match my_shmem.rlock() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire read lock !"),
        };

        //Check shared memory
        if shared_state.num_listenners > 0 {
            println!("We have a listenner !");
            break;
        }

        //Release the lock before sleeping
        drop(shared_state);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    //Modify the shared memory just for fun
    {
        let mut shared_state: WriteLockGuard<SharedState> = match my_shmem.wlock() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        let src = format!("Goodbye {} listenner(s) !\x00", shared_state.num_listenners);
        shared_state.message[0..src.len()].copy_from_slice(&src.as_bytes());
    }
}
