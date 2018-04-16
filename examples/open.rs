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

    {
        let buffer = match mem_file.read_lock() {
            Ok(v) => v.data,
            Err(_) => return,
        };

        print!("buffer = \"");
        for b in &buffer[0..16] {
            print!("\\x{:02x}", b);
        }
        println!("\"");
    }

    {
        let buffer_v: &mut [u8] = match mem_file.write_lock() {
            Ok(v) => v.data,
            Err(_) => return,
        };

        buffer_v[0] = 0x1;
    }

    std::thread::sleep(std::time::Duration::from_secs(2));

    {
        let buffer = match mem_file.read_lock() {
            Ok(v) => v.data,
            Err(_) => return,
        };

        print!("buffer = \"");
        for b in &buffer[0..16] {
            print!("\\x{:02x}", b);
        }
        println!("\"");
    }

    println!("Done");
}
