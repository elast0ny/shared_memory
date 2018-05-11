extern crate shared_memory;
use shared_memory::*;
use std::path::PathBuf;
use std::str::from_utf8_unchecked;

//Is there a rust function that does this ?
fn from_ut8f_to_null(bytes: &[u8], max_len: usize) -> &str {
    for i in 0..max_len {
        if bytes[i] == 0 {
            return unsafe {from_utf8_unchecked(&bytes[0..i])};
        }
    }
    panic!("Couldnt find null terminator.");
}

fn main() {

    //Open an existing shared SharedMem
    let mut my_shmem: SharedMem = match SharedMem::open_link(PathBuf::from("shared_mem.link")) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to open SharedMem...");
            return;
        }
    };

    println!("Openned link file with info : {}", my_shmem);

    println!("Trying to acquire read lock !");
    //Read the original contents
    {
        //Acquire read lock
        let read_buf: ReadLockGuardSlice<u8> = match my_shmem.rlock_as_slice(0) {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire read lock !"),
        };

        print!("Shared buffer = \"");
        print!("{}", from_ut8f_to_null(&read_buf[4..], 256));
        println!("\"");

        //This should block any other reader when LockType::Mutex is used,
        //When LockType::RwLock is used, multiple readers should be able to hold this for 5 seconds.
        println!("Holding read lock for 5 seconds !");
        std::thread::sleep(std::time::Duration::from_secs(5));
    }

    println!("Incrementing shared listenner count !");
    //Update the shared memory
    {
        let mut num_listenners: WriteLockGuard<u32> = match my_shmem.wlock(0) {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };
        *(*num_listenners) = 1;
    }

    //Read the contents of the buffer again
    std::thread::sleep(std::time::Duration::from_secs(1));
    {
        let read_buf = match my_shmem.rlock_as_slice::<u8>(0) {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire read lock !"),
        };

        print!("Shared buffer = \"");
        print!("{}", from_ut8f_to_null(&read_buf[4..], 256));
        println!("\"");
    }
}
