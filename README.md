# shared_memory

[![Build Status](https://travis-ci.org/elast0ny/shared_memory-rs.svg?branch=master)](https://travis-ci.org/elast0ny/shared_memory-rs)
[![crates.io](https://img.shields.io/crates/v/shared_memory.svg)](https://crates.io/crates/shared_memory)
[![mio](https://docs.rs/shared_memory/badge.svg)](https://docs.rs/shared_memory/)
![Lines of Code](https://tokei.rs/b1/github/elast0ny/shared_memory-rs)

A user friendly crate that allows you to share memory between __processes__.

## Usage

Writer based on [examples/create.rs](examples/create.rs)
``` rust
//Creates a new SharedMem link "shared_mem.link" that points to shared memory of size 4096
let mut my_shmem: SharedMem = match SharedMem::create(
  PathBuf::from("shared_mem.link"),
  LockType::Mutex, //Concurent accesses will be managed by a mutex
  4096
).unwrap();

//Acquire write lock
{
    let mut shared_data: WriteLockGuardSlice<u8> = match my_shmem.wlock_as_slice().unwrap();
    let src = b"Hello World !\x00";
    shared_data[0..src.len()].copy_from_slice(src);
}
```

Reader based on [examples/open.rs](examples/open.rs)
``` rust
// Open an existing SharedMem link named "shared_mem.link"
let mut my_shmem: SharedMem = match SharedMem::open(PathBuf::from("shared_mem.link")).unwrap();
//Aquire Read lock
{
   let mut shared_data = match my_shmem.rlock_as_slice::<u8>().unwrap();
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
|SharedMem.create/open|Create/open a SharedMem|✔|✔|✔|
|SharedMem.*_raw|Create/open a raw shared memory map|✔|✔|✔|
|LockType::Mutex|Mutually exclusive lock|✔|✔</sup>|✔|
|LockType::RwLock|Exlusive write/shared read|✔|X<sup>[2]</sup>|✔|

<sup>[1] I do not own a Mac so cannot properly test this library other than building against OSX.</sup>

<sup>[2] Windows provides no default implementation of Rwlock that is safe to share between processes. See [Issue #1](https://github.com/elast0ny/shared_memory-rs/issues/1)</sup>

## License

 * [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
 * [MIT license](http://opensource.org/licenses/MIT)

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
