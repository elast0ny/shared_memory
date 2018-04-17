extern crate mem_file;
use mem_file::*;

use std::path::PathBuf;

fn main() {

    let my_path: PathBuf = PathBuf::from("test.txt");
    let my_perms: MemPermission = MemPermission {read:true, write:true, execute:false};

    match std::fs::remove_file(&my_path) {_=>{},};

    let mut mem_file: MemFile = match MemFile::create(my_path.clone(), my_perms, 4096) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to create MemFile at \"{}\"...", my_path.to_string_lossy());
            return;
        }
    };

    println!("Waiting for non-zero !");

    loop {
        let shared_mem: &[u8] = match mem_file.rlock_as_slice() {
            Ok(v) => *v,
            Err(_) => return,
        };

        if shared_mem[0] == 0x1 { break;}
        drop(shared_mem);

        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    {
        /* TODO : Make this work through lifetime magic
        let buffer: &mut [u8] = match mem_file.wlock_as_slice() {
            Ok(v) => &mut *v,
            Err(_) => return,
        };
        */

        let mut wlock = match mem_file.wlock_as_slice::<u8>() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };
        let write_buf: &mut [u8] = &mut *wlock;

        write_buf[0] = 0x41;
        write_buf[1] = 0x41;
        write_buf[2] = 0x41;
        write_buf[3] = 0x41;
        write_buf[4] = 0x41;
        write_buf[5] = 0x41;
    }

    println!("Done !");
}
