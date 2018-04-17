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
        let buffer: &[u8] = match mem_file.rlock_as_slice() {
            Ok(v) => *v,
            Err(_) => return,
        };

        print!("Orig buffer = \"");
        for b in &buffer[0..16] {
            print!("\\x{:02x}", b);
        }
        println!("\"");
    }

    {
        let mut wlock = match mem_file.wlock_as_slice::<u8>() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };
        let write_buf: &mut [u8] = &mut *wlock;
        println!("write_buf[0] = 0x1");
        write_buf[0] = 0x1;
    }

    std::thread::sleep(std::time::Duration::from_secs(1));

    {
        let buffer = match mem_file.rlock_as_slice::<u8>() {
            Ok(v) => *v,
            Err(_) => return,
        };

        print!("After buffer = \"");
        print!("{}", unsafe {std::str::from_utf8_unchecked(buffer)});
        println!("\"");
    }

    println!("Done");
}
