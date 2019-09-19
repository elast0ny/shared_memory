use ::shared_memory::*;
use std::ffi::{CStr, CString};

#[derive(SharedMemCast)]
struct ShmemStructExample {
    num_slaves: u32,
    message: [u8; 256],
}

static GLOBAL_LOCK_ID: usize = 0;

fn main() -> Result<(), SharedMemError> {
    println!("Attempting to create/open custom shmem !");
    let shmem = match SharedMem::create_linked("shared_mem.link", LockType::Mutex, 4096) {
        // We created and own this mapping
        Ok(v) => v,
        // Link file already exists
        Err(SharedMemError::LinkExists) => SharedMem::open_linked("shared_mem.link")?,
        Err(e) => return Err(e),
    };

    println!("Mapping info : {}", shmem);

    if shmem.num_locks() != 1 {
        println!("Expected to only have 1 lock in shared mapping !");
        return Err(SharedMemError::InvalidHeader);
    }

    if shmem.is_owner() {
        master(shmem)
    } else {
        slave(shmem)
    }
}

fn slave(mut shmem: SharedMem) -> Result<(), SharedMemError> {
    println!("[S] Reading string from shared memory...");
    // Scope to ensure proper Drop of lock
    {
        let shared_state = shmem.rlock::<ShmemStructExample>(GLOBAL_LOCK_ID)?;
        let shmem_str: &CStr = unsafe { CStr::from_ptr(shared_state.message.as_ptr() as *mut i8) };
        println!("[S]\tShared message : {}", shmem_str.to_string_lossy());
    }

    println!("[S] Incrementing shared listenner count !");
    // Scope to ensure proper Drop of lock
    {
        let mut shared_state = shmem.wlock::<ShmemStructExample>(GLOBAL_LOCK_ID)?;
        shared_state.num_slaves += 1;
        println!("[S]\tDone");
    }

    println!("[S] Sleeping for 10 seconds !");
    std::thread::sleep(std::time::Duration::from_secs(10));

    println!("[S] Reading string from shared memory...");
    // Scope to ensure proper Drop of lock
    {
        let shared_state = shmem.rlock::<ShmemStructExample>(GLOBAL_LOCK_ID)?;
        let shmem_str: &CStr = unsafe { CStr::from_ptr(shared_state.message.as_ptr() as *mut i8) };
        println!("[S]\tShared message : {}", shmem_str.to_string_lossy());
    }

    Ok(())
}

fn master(mut shmem: SharedMem) -> Result<(), SharedMemError> {
    println!("[M] Writting string in shared memory...");
    // Scope to ensure proper Drop of lock
    {
        let mut shared_state = shmem.wlock::<ShmemStructExample>(GLOBAL_LOCK_ID)?;
        let default_string: CString =
            CString::new("This is a string written my the master !").unwrap();

        // Write null terminated string into the shared memory
        shared_state.message[0..default_string.to_bytes_with_nul().len()]
            .copy_from_slice(default_string.to_bytes_with_nul());
        println!("[M]\tDone");

        println!("[M] Holding global lock for 5 seconds");
        std::thread::sleep(std::time::Duration::from_secs(5));
    }

    println!("[M] Monitoring slave count for the next 5 seconds...");
    let poll_interval_ms: usize = 200;
    let mut sleep_count: usize = 0;
    let mut last_slave_count: u32 = 0;
    loop {
        let shared_state = shmem.rlock::<ShmemStructExample>(GLOBAL_LOCK_ID)?;

        if shared_state.num_slaves != last_slave_count {
            println!("[M]\tWe now have {} slave(s) !", shared_state.num_slaves);
            last_slave_count = shared_state.num_slaves;
        }

        // Release global lock asap
        drop(shared_state);

        std::thread::sleep(std::time::Duration::from_millis(poll_interval_ms as u64));
        sleep_count += 1;

        if sleep_count * poll_interval_ms >= 5000 {
            println!("[M]\tFinal slave count : {}", last_slave_count);
            break;
        }
    }

    println!("[M] Changing string in shared memory...");
    // Scope to ensure proper Drop of lock
    {
        let mut shared_state = shmem.wlock::<ShmemStructExample>(GLOBAL_LOCK_ID)?;
        let default_string: CString =
            CString::new(format!("Goodbye {} slave(s)", shared_state.num_slaves)).unwrap();

        // Write null terminated string into the shared memory
        shared_state.message[0..default_string.to_bytes_with_nul().len()]
            .copy_from_slice(default_string.to_bytes_with_nul());
        println!("[M]\tDone");
    }

    println!("[M] Exiting");

    Ok(())
}
