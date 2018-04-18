extern crate mem_file;
use mem_file::*;
use std::path::PathBuf;

fn main() {

    //Create a new MemFile of size 4096
    let mut mem_file: MemFile = match MemFile::create(PathBuf::from("test.txt"), 4096) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to create MemFile...");
            return;
        }
    };

    {
        let mut write_buf: MemFileWLockSlice<u8> = match mem_file.wlock_as_slice() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        let src = b"Hello World !\x00";
        write_buf[0..src.len()].copy_from_slice(src);

        println!("Wrote : {}", unsafe {std::str::from_utf8_unchecked(*write_buf)})
    }

    println!("Waiting for first byte of shared memory to change...");

    //Loop until our memory has changed
    loop {

        //Acquire read lock
        let read_buf: MemFileRLockSlice<u8> = match mem_file.rlock_as_slice() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        //Check shared memory
        if read_buf[0] != 0x48 {
            println!("First byte has changed to 0x{:x} !", read_buf[0]);
            //This will also drop the lock
            break;
        }

        //Release the lock before sleeping
        drop(read_buf);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    println!("Writing some data...");

    //Modify the shared memory just for fun
    {
        let mut write_buf: MemFileWLockSlice<u8> = match mem_file.wlock_as_slice() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        let src = b"Bye !\x00";
        write_buf[0..src.len()].copy_from_slice(src);
    }

    println!("Done !");
}
