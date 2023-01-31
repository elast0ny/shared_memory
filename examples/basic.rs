use std::thread;

use clap::Parser;
use shared_memory::*;

/// Spawns N threads that increment a value to 100
#[derive(Parser)]
#[clap(author, version, about)]
struct Args {
    /// Number of threads to spawn
    num_threads: usize,

    /// Count to this value
    #[clap(long, short, default_value_t = 50)]
    count_to: u8,
}

fn main() {
    env_logger::init();
    let args = Args::parse();

    if args.num_threads < 1 {
        eprintln!("Invalid number of threads");
        return;
    }

    let mut threads = Vec::with_capacity(args.num_threads);
    let _ = std::fs::remove_file("basic_mapping");
    let max = args.count_to;
    // Spawn N threads
    for i in 0..args.num_threads {
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
            eprintln!("Unable to create or open shmem flink {shmem_flink} : {e}");
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
