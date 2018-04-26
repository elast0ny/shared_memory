extern crate mem_file;
use mem_file::*;
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

    //Open an existing shared MemFile
    let mut mem_file: MemFile = match MemFile::open(PathBuf::from("shared_mem.link")) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to open MemFile...");
            return;
        }
    };

    println!("Openned link file \"{}\"
    Backed by OS identifier : \"{}\"
    Size : 0x{:x}",
    mem_file.get_link_path().unwrap().to_string_lossy(),
    mem_file.get_real_path().unwrap(),
    mem_file.get_size());

    println!("Trying to acquire read lock !");
    //Read the original contents
    {
        //Acquire read lock
        let read_buf: ReadLockGuardSlice<u8> = match mem_file.rlock_as_slice() {
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
        let mut num_listenners: WriteLockGuard<u32> = match mem_file.wlock() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };
        *(*num_listenners) = 1;
    }

    //Read the contents of the buffer again
    std::thread::sleep(std::time::Duration::from_secs(1));
    {
        let read_buf = match mem_file.rlock_as_slice::<u8>() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire read lock !"),
        };

        print!("Shared buffer = \"");
        print!("{}", from_ut8f_to_null(&read_buf[4..], 256));
        println!("\"");
    }
}
