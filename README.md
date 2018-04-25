# mem_file

[![Build Status](https://travis-ci.org/elast0ny/mem_file.svg?branch=master)](https://travis-ci.org/elast0ny/mem_file)
[![crates.io](https://img.shields.io/crates/v/mem_file.svg)](https://crates.io/crates/mem_file)
[![mio](https://docs.rs/mem_file/badge.svg)](https://docs.rs/mem_file/)
![Lines of Code](https://tokei.rs/b1/github/elast0ny/mem_file)

This crate provides a simple interface to shared memory OS APIs.

Shared memory is well suited for sharing large amounts of data between processes as it relies purely on memory accesses. Other than when managing concurent access through locks/events, reading and writing memory from a MemFile relies only on CPU features (the operating system is not involved, no context switches like system calls, etc...).

## Usage

Writer based on [examples/create.rs](examples/create.rs)
``` rust
//Creates a new MemFile link "shared_mem.link" that points to shared memory of size 4096
let mut mem_file: MemFile = match MemFile::create(
  PathBuf::from("shared_mem.link"),
  LockType::Mutex, //Concurent accesses will be managed by a mutex
  4096
).unwrap();

//Acquire write lock
{
    let mut shared_data: WriteLockGuardSlice<u8> = match mem_file.wlock_as_slice().unwrap();
    let src = b"Hello World !\x00";
    shared_data[0..src.len()].copy_from_slice(src);
}
```

Reader based on [examples/open.rs](examples/open.rs)
``` rust
// Open an existing MemFile link named "shared_mem.link"
let mut mem_file: MemFile = match MemFile::open(PathBuf::from("shared_mem.link")).unwrap();
//Aquire Read lock
{
   let mut shared_data = match mem_file.rlock_as_slice::<u8>().unwrap();
   //Print the content of the shared memory as chars
   for byte in &shared_data[0..256] {
       if *byte == 0 { break; }
       print!("{}", *byte as char);
   }
}
```

## Operating System Support

| Feature| Description | Linux | Windows|  Mac<sup>[1]</sup>|
|--------|-------------|:-----:|:------:|:----:|
|MemFile.create/open|Create/open a MemFile|✔|✔|X|
|MemFile.*_raw|Create/open a raw shared memory map|✔|✔|X|
|LockType::Mutex|Mutually exclusive lock|✔|X<sup>[2]</sup>|X|
|LockType::RwLock|Exlusive write/shared read|✔|X<sup>[3]</sup>|X|

<sup>[1] I do not own a Mac so cannot implement that side of things myself. Contributions are welcome !</sup>

<sup>[2] Rust winapi crate [does not implement any synchronization functions](https://github.com/retep998/winapi-rs/issues/609)</sup>

<sup>[3] Windows provides no default implementation of Rwlock that is safe to share between processes. See [Issue #1](https://github.com/elast0ny/mem_file/issues/1)</sup>

## License

 * [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
 * [MIT license](http://opensource.org/licenses/MIT)

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
