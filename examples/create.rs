extern crate mem_file;
use mem_file::*;

use std::path::PathBuf;

fn main() {

    let my_path: PathBuf = PathBuf::from("C:\\Users\\Tony\\Downloads\\test.txt");
    let my_perms: MemPermission = MemPermission {read:true, write:true, execute:false};

    match std::fs::remove_file(&my_path) {_=>{},};

    let mut mem_file: MemFile = match MemFile::create(&my_path, my_perms, 4096) {
        Ok(v) => v,
        Err(e) => {
            println!("Failed to create \"{}\"", my_path.to_string_lossy());
            println!("Error : {}", e);
            return;
        }
    };

    let shared_mem = mem_file.mem_addr.as_mut().unwrap();
    while shared_mem[0] == 0 {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    println!("TEST !");
}
