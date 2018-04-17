extern crate mem_file;
use mem_file::*;
use std::path::PathBuf;

fn main() {

    //Open an existing MemFile
    let mut mem_file: MemFile = match MemFile::open(PathBuf::from("test.txt")) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to open MemFile...");
            return;
        }
    };

    //Read the original contents
    {
        //Acquire read lock
        let read_buf: MemFileRLockSlice<u8> = match mem_file.rlock_as_slice() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        print!("Orig buffer = \"");
        print!("{}", std::str::from_utf8(*read_buf).unwrap());
        println!("\"");
    }

    //Write 0x01 in the first byte
    {
        let mut write_buf: MemFileWLockSlice<u8> = match mem_file.wlock_as_slice() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        println!("write_buf[0] = 0x1");
        write_buf[0] = 0x1;
    }

    //Read the contents of the buffer again
    std::thread::sleep(std::time::Duration::from_secs(1));
    {
        let read_buf = match mem_file.rlock_as_slice::<u8>() {
            Ok(v) => v,
            Err(_) => panic!("Failed to acquire write lock !"),
        };

        print!("After buffer = \"");
        print!("{}", std::str::from_utf8(*read_buf).unwrap());
        println!("\"");
        println!("len 0x{:x}", read_buf.len())
    }

    println!("Done");
}
