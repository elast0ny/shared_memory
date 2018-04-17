extern crate mem_file;
use mem_file::*;

use std::path::PathBuf;

fn main() {

    let my_path: PathBuf = PathBuf::from("test.txt");
    let my_perms: MemPermission = MemPermission {read:true, write:true, execute:false};

    //Create a new RW- MemFile of size 4096
    let mut mem_file: MemFile = match MemFile::create(my_path.clone(), my_perms, 4096) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to create MemFile at \"{}\"...", my_path.to_string_lossy());
            return;
        }
    };

    println!("Waiting for first byte of shared memory to change...");

    //Loop until our memory has changed
    loop {

        //Acquire read lock
        let shared_mem: &[u8] = match mem_file.rlock_as_slice() {
            Ok(v) => *v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        //Read shared memory
        if shared_mem[0] == 0x1 {
            //This will also drop the lock
            break;
        }

        //Release the lock before sleeping
        drop(shared_mem);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    println!("Byte has changed !\nWriting some data...");

    //Modify the shared memory just for fun
    {
        /* TODO : Make this work somehow
        let buffer: &mut [u8] = match mem_file.wlock_as_slice() {
            Ok(v) => &mut *v,
            Err(_) => return,
        };
        */

        let mut wlock = match mem_file.wlock_as_slice::<char>() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        let write_buf: &mut [char] = &mut *wlock;

        write_buf[0] = 'B';
        write_buf[1] = 'y';
        write_buf[2] = 'e';
    }

    println!("Done !");
}
