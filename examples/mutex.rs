use raw_sync::locks::*;
use shared_memory::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Attempt to create a mapping or open if it already exists
    println!("Getting the shared memory mapping");

    let shmem = match ShmemConf::new().size(4096).flink("event_mapping").create() {
        Ok(m) => m,
        Err(ShmemError::LinkExists) => ShmemConf::new().flink("event_mapping").open()?,
        Err(e) => return Err(Box::new(e)),
    };

    if shmem.is_owner() {
        //Create an event in the shared memory
        println!("Creating mutex in shared memory");
        let base_ptr = shmem.as_ptr();
        let (mutex, _) =
            unsafe { Mutex::new(base_ptr, base_ptr.add(Mutex::size_of(Some(base_ptr))))? };

        println!("Incrementing value by 1 !");
        loop {
            let mut guard = mutex.lock()?;
            let val: &mut u8 = unsafe { &mut **guard };
            if *val > 10 {
                break;
            }
            println!("Val : {}", *val);
            *val += 1;
            mutex.release()?;
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    } else {
        // Open existing event
        println!("Openning mutex from shared memory");
        let base_ptr = shmem.as_ptr();
        let (mutex, _) = unsafe {
            Mutex::from_existing(base_ptr, base_ptr.add(Mutex::size_of(Some(base_ptr))))?
        };

        println!("Incrementing value by 2 !");
        loop {
            let mut guard = mutex.lock()?;
            let val: &mut u8 = unsafe { &mut **guard };
            if *val > 10 {
                break;
            }
            println!("Val : {}", *val);
            *val += 2;
            mutex.release()?;
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }

    println!("Done !");

    Ok(())
}
