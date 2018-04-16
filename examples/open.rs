extern crate mem_file;
use mem_file::*;

use std::path::PathBuf;

fn main() {

    let my_path: PathBuf = PathBuf::from("test.txt");
    let my_perms: MemPermission = MemPermission {
        read:true,
        write:true,
        execute:true};

    let mut mem_file: MemFile = match MemFile::open(my_path.clone(), my_perms) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to open \"{}\"", my_path.to_string_lossy());
            return;
        }
    };


    if let Some(shared_mem) = mem_file.get_mut_nolock() {
        println!("Setting *{:p} to non-zero !", &(shared_mem[0]));
        shared_mem[0] = 0x1;
    }

    println!("Done");
}
