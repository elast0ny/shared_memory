use raw_sync::{events::*, Timeout};
use shared_memory::*;
use log::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    // Attempt to create a mapping or open if it already exists
    info!("Getting the shared memory mapping");
    let shmem = match ShmemConf::new().size(4096).flink("event_mapping").create() {
        Ok(m) => m,
        Err(ShmemError::LinkExists) => ShmemConf::new().flink("event_mapping").open()?,
        Err(e) => return Err(Box::new(e)),
    };

    if shmem.is_owner() {
        //Create an event in the shared memory
        info!("Creating event in shared memory");
        let (evt, used_bytes) = unsafe { Event::new(shmem.as_ptr(), true)? };
        info!("\tUsed {} bytes", used_bytes);

        info!("Launch another instance of this example to signal the event !");
        evt.wait(Timeout::Infinite)?;
        info!("\tGot signal !");
    } else {
        // Open existing event
        info!("Openning event from shared memory");
        let (evt, used_bytes) = unsafe { Event::from_existing(shmem.as_ptr())? };
        info!("\tEvent uses {} bytes", used_bytes);

        info!("Signaling event !");
        evt.set(EventState::Signaled)?;
        info!("\tSignaled !");
    }

    info!("Done !");
    Ok(())
}
