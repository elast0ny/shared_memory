extern crate mem_file;
use mem_file::*;
use std::path::PathBuf;

fn main() {

    //Open an existing raw MemFile
    let mut mem_file: MemFile = match MemFile::open_raw(String::from("some_raw_map")) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to open MemFile...");
            return;
        }
    };

    println!("Openned link file \"{}
    Backed by OS identifier : \"{}\"
    Size : 0x{:x}",
    mem_file.get_link_path().unwrap_or(&PathBuf::from("[NONE]")).to_string_lossy(),
    mem_file.get_real_path().unwrap(),
    mem_file.get_size());

    println!("Swapping first byte to 0x1 !");

    //Update the shared memory
    {
        let mut num_listenners: WriteLockGuard<u8> = match mem_file.wlock() {
            Ok(v) => v,
            Err(e) => panic!("ERROR : {}\nFailed to acquire write lock !", e),
        };
        *(*num_listenners) = 0x1;
    }

    println!("Done !");
}
