use raw_sync::{events::*, Timeout};
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
        println!("Creating event in shared memory");
        let (evt, used_bytes) = unsafe { Event::new(shmem.as_ptr(), true)? };
        println!("\tUsed {} bytes", used_bytes);

        println!("Launch another instance of this example to signal the event !");
        evt.wait(Timeout::Infinite)?;
        println!("\tGot signal !");
    } else {
        // Open existing event
        println!("Openning event from shared memory");
        let (evt, used_bytes) = unsafe { Event::from_existing(shmem.as_ptr())? };
        println!("\tEvent uses {} bytes", used_bytes);

        println!("Signaling event !");
        evt.set(EventState::Signaled)?;
        println!("\tSignaled !");
    }

    println!("Done !");
    Ok(())
}
