extern crate mem_file;
use mem_file::*;

use std::path::PathBuf;

fn main() {

    //Create a new RW- MemFile of size 4096
    let mut mem_file: MemFile = match MemFile::create(PathBuf::from("test.txt"), 4096) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to create MemFile...");
            return;
        }
    };

    {
        let mut write_buf: MemFileWLockSlice<char> = match mem_file.wlock_as_slice() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        write_buf[0] = 'H';
        write_buf[1] = 'e';
        write_buf[2] = 'l';
        write_buf[3] = 'l';
        write_buf[4] = 'o';
        write_buf[5] = '\x00';
        write_buf[6] = '\x00';
    }

    println!("Waiting for first byte of shared memory to change...");

    //Loop until our memory has changed
    loop {

        //Acquire read lock
        let read_buf: MemFileRLockSlice<char> = match mem_file.rlock_as_slice() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        //Check shared memory
        if read_buf[0] != 'H' {
            //This will also drop the lock
            break;
        }

        //Release the lock before sleeping
        drop(read_buf);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    println!("Byte has changed !\nWriting some data...");

    //Modify the shared memory just for fun
    {
        let mut write_buf: MemFileWLockSlice<char> = match mem_file.wlock_as_slice() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        write_buf[0] = 'B';
        write_buf[1] = 'y';
        write_buf[2] = 'e';

        write_buf[3] = '\x00';
        write_buf[4] = '\x00';
    }

    println!("Done !");
}
