# mem_file

Provides a wrapper around native shared memory for [Linux](http://man7.org/linux/man-pages/man7/shm_overview.7.html) and [Windows](http://lmgtfy.com/?q=shared+memory+windows).

This crate is ideal if you need to share large amounts of data with another process purely through memory.

[Documentation](https://docs.rs/mem_file/) | [crates.io](https://crates.io/crates/mem_file)

## Usage

Creator based on [examples/create.rs](examples/create.rs)
``` rust
//Create a MemFile at `pwd`\test.txt of size 4096
let mut mem_file: MemFile = match MemFile::create(PathBuf::from("test.txt"), 4096) {<...>};
//Set explicit scope for the lock (no need to call drop(shared_data))
{
   //Acquire write lock
   let mut shared_data = match mem_file.wlock_as_slice::<u8>() {<...>};
   let src = b"Some string you want to share\x00";
   //Write to the shared memory
   shared_data[0..src.len()].copy_from_slice(src);
}
```

Slave based on [examples/open.rs](examples/open.rs)
``` rust
// Open an existing MemFile from `pwd`\test.txt
let mut mem_file: MemFile = match MemFile::open(PathBuf::from("test.txt")) {<...>};
//Set explicit scope for the lock (no need to call drop(shared_data))
{
   //Acquire read lock
   let mut shared_data = match mem_file.rlock_as_slice::<u8>() {<...>};
   //Print the content of the shared memory as chars
   for byte in &shared_data[0..256] {
       if *byte == 0 { break; }
       print!("{}", *byte as char);
   }
}
```

## License

Licensed under either of

 * [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
 * [MIT license](http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
