use ::shared_memory::*;
use std::sync::atomic::*;

fn main() -> Result<(), SharedMemError> {
    println!("Attempting to create/open custom shmem !");
    let shmem = match SharedMemRaw::create("some_raw_map", 4096) {
        // We created and own this mapping
        Ok(v) => v,
        // Link file already exists
        Err(SharedMemError::MappingIdExists) => SharedMemRaw::open("some_raw_map")?,
        Err(e) => return Err(e),
    };

    println!(
        "Raw mapping info :
        \tOs ID : \"{}\"
        \tSize : 0x{:x}",
        shmem.get_path(),
        shmem.get_size()
    );

    if shmem.is_owner() {
        master(shmem)
    } else {
        slave(shmem)
    }
}

fn slave(mut shmem: SharedMemRaw) -> Result<(), SharedMemError> {
    println!("[S] Swapping first byte to 0x1 !");

    //Update the shared memory
    let first_byte: &mut AtomicBool = unsafe { shmem.get_raw_mut() };
    first_byte.store(true, Ordering::Relaxed);

    println!("[S]\tDone !");

    Ok(())
}

fn master(shmem: SharedMemRaw) -> Result<(), SharedMemError> {
    
    println!("[M] Busy looping until first byte changes...");

    let first_byte: &AtomicBool = unsafe { shmem.get_raw() };

    while !first_byte.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    println!("[M]\tDone !");

    Ok(())
}
