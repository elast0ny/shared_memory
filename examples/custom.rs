use ::shared_memory::*;

static MASTER_EVT_ID: usize = 0;
static SLAVE_EVT_ID: usize = 1;

fn main() -> Result<(), SharedMemError> {
    let my_conf = SharedMemConf::default()
        .set_link_path("shared_mem.link")
        .set_os_path("test_mapping")
        .set_size(4096)
        .add_event(EventType::Auto)? // MASTER_EVT_ID
        .add_event(EventType::Auto)?; // SLAVE_EVT_ID

    println!("Attempting to create/open custom shmem !");
    let shmem = match my_conf.create() {
        // We created and own this mapping
        Ok(v) => v,
        // Link file already exists
        Err(SharedMemError::LinkExists) => SharedMem::open_linked("shared_mem.link")?,
        Err(e) => return Err(e),
    };

    println!("Mapping info : {}", shmem);

    if shmem.is_owner() {
        master(shmem)
    } else {
        slave(shmem)
    }
}

fn slave(mut shmem: SharedMem) -> Result<(), SharedMemError> {
    println!("[S] Sending signal to master");
    shmem.set(MASTER_EVT_ID, EventState::Signaled)?;

    println!("[S] Waiting for master to signal for 10s");
    shmem.wait(SLAVE_EVT_ID, Timeout::Sec(10))?;
    println!("[S]\t Got Signal !");

    println!("[S] Waiting for master to signal for 10s");
    shmem.wait(SLAVE_EVT_ID, Timeout::Sec(10))?;
    println!("[S]\t Got Signal !");

    println!("[S] Sending signal to master");
    shmem.set(MASTER_EVT_ID, EventState::Signaled)?;

    println!("[S] Done !");

    Ok(())
}

fn master(mut shmem: SharedMem) -> Result<(), SharedMemError>  {
    
    println!("[M] Waiting for slave to send us a signal");
    shmem.wait(MASTER_EVT_ID, Timeout::Sec(5))?;
    println!("[M]\t Got Signal !");

    println!("[M] Sending signal to slave");
    shmem.set(SLAVE_EVT_ID, EventState::Signaled)?;

    println!("[M] Sleeping for 5s");
    std::thread::sleep(std::time::Duration::from_secs(5));

    println!("[M] Sending signal to slave");
    shmem.set(SLAVE_EVT_ID, EventState::Signaled)?;

    println!("[M] Waiting for slave to send us a signal");
    shmem.wait(MASTER_EVT_ID, Timeout::Sec(5))?;
    println!("[M]\t Got Signal !");

    println!("[M] Done !");

    Ok(())
}