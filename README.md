# shared_memory
[![Build Status](https://github.com/elast0ny/shared_memory-rs/workflows/build/badge.svg)](https://github.com/elast0ny/shared_memory-rs/actions?query=workflow%3Abuild)
[![crates.io](https://img.shields.io/crates/v/shared_memory.svg)](https://crates.io/crates/shared_memory)
[![mio](https://docs.rs/shared_memory/badge.svg)](https://docs.rs/shared_memory/)
[![Lines of Code](https://tokei.rs/b1/github/elast0ny/shared_memory-rs?category=code)](https://tokei.rs/b1/github/elast0ny/shared_memory-rs?category=code)

A crate that allows you to share memory between __processes__.

This crate provides lightweight wrappers around shared memory APIs in an OS agnostic way. It is intended to be used with it's sister crate [raw_sync](https://github.com/elast0ny/raw_sync-rs) which provide simple primitves to synchronize access to the shared memory (Mutex, RwLock, Events, etc...).

| raw_sync |
|----|
|[![crates.io](https://img.shields.io/crates/v/raw_sync.svg)](https://crates.io/crates/raw_sync) [![docs.rs](https://docs.rs/raw_sync/badge.svg)](https://docs.rs/raw_sync/)|

## Usage

For usage examples, see code located in [examples/](examples/) :

  | Examples | Description |
  |----------|-------------|
  |[event](examples/event.rs)| Shows the use of shared events through shared memory|
  |[mutex](examples/event.rs)| Shows the use of a shared mutex through shared memory|

## License

 * [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
 * [MIT license](http://opensource.org/licenses/MIT)

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.


## Changelog

### __0.12.X__
- Windows implementation now follows POSIX behavior in regards to ownership and deletion, see [#59](https://github.com/elast0ny/shared_memory-rs/pull/59) for more details
- Added feature gated debug logging ("logging" feature has to be enabled explicitly)
### __0.11.X__
This release breaks backwards compatibility and removes a bunch of previous features which hid many unsafe behaviors (automatically casting shared memory to Rust types).

The release also marks the split between `shared_memory` and its synchronization primitives into a seperate crate `raw_sync`.
