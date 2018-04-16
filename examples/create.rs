extern crate mem_file;
use mem_file::*;

use std::path::PathBuf;

fn main() {

    let my_path: PathBuf = PathBuf::from("test.txt");
    let my_perms: MemPermission = MemPermission {read:true, write:true, execute:false};

    match std::fs::remove_file(&my_path) {_=>{},};

    let mem_file: MemFile = match MemFile::create(my_path.clone(), my_perms, 4096) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to create MemFile at \"{}\"...", my_path.to_string_lossy());
            return;
        }
    };

    if let Some(shared_mem) = mem_file.get_mut_nolock() {
        println!("Waiting for *{:p} to be non-zero !", &(shared_mem[0]));
        while shared_mem[0] == 0 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    println!("Done !");
}
