use std::thread;

use clap::{App, Arg};
use shared_memory::*;

fn main() {
    env_logger::init();

    // Get number of thread argument
    let matches = App::new("Basic Example")
        .about("Spawns N threads that increment a value to 100")
        .arg(
            Arg::with_name("num_threads")
                .help("Number of threads to spawn")
                .required(true)
                .takes_value(true),
        )
        .arg(
            Arg::with_name("count_to")
                .help("Count to this value")
                .short("c")
                .long("count")
                .default_value("50")
                .takes_value(true)
        )
        .get_matches();
    let num_threads: usize = matches
        .value_of("num_threads")
        .unwrap()
        .parse()
        .expect("Invalid number passed for num_threads");

    let max: u8 = matches
        .value_of("count_to")
        .unwrap()
        .parse()
        .expect("Invalid number passed for count_to");
    
    if num_threads < 1 {
        eprintln!("Invalid number of threads");
        return;
    }

    let mut threads = Vec::with_capacity(num_threads);
    let _ = std::fs::remove_file("basic_mapping");

    // Spawn N threads
    for i in 0..num_threads {
        let thread_id = i + 1;
        threads.push(thread::spawn(move || {
            increment_value("basic_mapping", thread_id, max);
        }));
    }

    // Wait for threads to exit
    for t in threads.drain(..) {
        t.join().unwrap();
    }
}

/// Increments a value that lives in shared memory
fn increment_value(shmem_flink: &str, thread_num: usize, max: u8) {
    // Create or open the shared memory mapping
    let shmem = match ShmemConf::new().size(4096).flink(shmem_flink).create() {
        Ok(m) => m,
        Err(ShmemError::LinkExists) => ShmemConf::new().flink(shmem_flink).open().unwrap(),
        Err(e) => {
            eprintln!(
                "Unable to create or open shmem flink {} : {}",
                shmem_flink, e
            );
            return;
        }
    };

    // Get pointer to the shared memory
    let raw_ptr = shmem.as_ptr();

    // WARNING: This is prone to race conditions as no sync/locking is used
    unsafe {
        while std::ptr::read_volatile(raw_ptr) < max {
            // Increment shared value by one
            *raw_ptr += 1;

            println!(
                "[thread:{}] {}",
                thread_num,
                std::ptr::read_volatile(raw_ptr)
            );

            // Sleep for a bit
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
}
