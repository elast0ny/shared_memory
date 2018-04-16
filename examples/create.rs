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
        let shared_mem: &[u8] = match mem_file.read_lock() {
            Ok(v) => v.data,
            Err(_e) => {return;},
        };
        if shared_mem[0] == 0x1 { break;}
        drop(shared_mem);

        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    {
        let shared_mem: &mut [u8] = match mem_file.write_lock() {
            Ok(v) => v.data,
            Err(_e) => {return;},
        };

        shared_mem[1] = 0x41;
        shared_mem[2] = 0x41;
        shared_mem[3] = 0x41;
        shared_mem[4] = 0x41;
        shared_mem[5] = 0x41;
    }

    println!("Done !");
}
