extern crate mem_file;
use mem_file::*;

use std::path::PathBuf;

fn main() {

    let my_path: PathBuf = PathBuf::from("C:\\Users\\Tony\\Downloads\\test.txt");
    let my_perms: MemPermission = MemPermission {read:true, write:true, execute:false};

    let mut mem_file: MemFile = match MemFile::open(&my_path, my_perms) {
        Ok(v) => v,
        Err(e) => {
            println!("Failed to open \"{}\"", my_path.to_string_lossy());
            println!("Error : {}", e);
            return;
        }
    };

    println!("Openned mapping @ {:p} size : {}",  mem_file.mem_addr.as_ref().unwrap(),  mem_file.mem_size);
    let shared_mem = mem_file.mem_addr.as_mut().unwrap();
    shared_mem[0] = 0x1;
    println!("Done");
}
