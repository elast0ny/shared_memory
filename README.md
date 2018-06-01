# shared_memory

[![Build Status](https://travis-ci.org/elast0ny/shared_memory-rs.svg?branch=master)](https://travis-ci.org/elast0ny/shared_memory-rs)
[![crates.io](https://img.shields.io/crates/v/shared_memory.svg)](https://crates.io/crates/shared_memory)
[![mio](https://docs.rs/shared_memory/badge.svg)](https://docs.rs/shared_memory/)
![Lines of Code](https://tokei.rs/b1/github/elast0ny/shared_memory-rs)

A user friendly crate that allows you to share memory between __processes__.

This crate aims to provide lightweight wrappers around shared memory mappings in an OS agnostic way while also providing an abstraction layer on comonly used [synchronization primitives](#synchronization-primitives).

## Usage

For usage examples, see code located in [examples/](examples/) :

  | Examples | Description |
  |----------|-------------|
  |simple_[[create](examples/simple_create.rs)&#124;[open](examples/simple_open.rs)]|Basic use of the library when all you need is memory protected by one lock|
  |custom_[[create](examples/custom_create.rs)&#124;[open](examples/custom_open.rs)] | Shows the more advanced usage of the crate with configs and events |
  |raw_[[create](examples/raw_create.rs)&#124;[open](examples/raw_create.rs)]| Create/Open raw mappings that are not managed by this crate |

## Synchronization Primitives

| Feature| Description | Linux | Windows|  Mac<sup>**</sup>| FreeBSD |
|--------|-------------|:-----:|:------:|:----:| :-----: |
|LockType::Mutex|Mutually exclusive lock|✔|✔</sup>|✔|✔|
|LockType::RwLock|Exlusive write/shared read|✔|X<sup>[#1](https://github.com/elast0ny/shared_memory-rs/issues/1)</sup>|✔|✔|
|EventType::Auto/Manual| Generic event : [pthread_cond](https://linux.die.net/man/3/pthread_cond_init) on unix and [Event Objects](https://msdn.microsoft.com/en-us/library/windows/desktop/ms682655.aspx) on windows. |✔|✔|X<sup>[#14](https://github.com/elast0ny/shared_memory-rs/issues/14)</sup>|X<sup>[#14](https://github.com/elast0ny/shared_memory-rs/issues/14)</sup>|
|EventType::*Busy|Busy event managed by polling an AtomicBool in a loop|✔|✔|✔|✔|
|EventType::*EventFd|[Linux specific event type](http://man7.org/linux/man-pages/man2/eventfd.2.html)|✔|N/A|N/A|N/A|

<sup>\* Events take the Auto or Manual prefix to indicate wether signals are automatical "consumed" by waiting threads or not.</sup>
<br><sup>\*\* I do not own a Mac (or FreeBSD) so cannot easily test this library other than building against the platform.</sup>

## License

 * [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
 * [MIT license](http://opensource.org/licenses/MIT)

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
