use shared_memory::ShmemConf;
use std::sync::mpsc::channel;
use std::thread;

#[test]
fn persistence() {
    let os_id = {
        let mut shmem = ShmemConf::new().size(4096).create().unwrap();
        shmem.set_owner(false);
        String::from(shmem.get_os_id())
    };
    let mut shmem = ShmemConf::new().os_id(os_id).open().unwrap();
    shmem.set_owner(true);
}

#[test]
fn posix_behavior() {
    let (tx_a, rx_a) = channel();
    let (tx_b, rx_b) = channel();
    let (tx_c, rx_c) = channel();

    let thread_a = thread::Builder::new()
        .name(String::from("A"))
        .spawn(move || {
            let os_id = {
                let shmem = ShmemConf::new().size(4096).create().unwrap();
                let os_id = String::from(shmem.get_os_id());
                // Creating two `Shmem`s with the same `os_id` should fail
                assert!(ShmemConf::new().size(4096).os_id(&os_id).create().is_err());
                tx_b.send(os_id.clone()).unwrap();
                tx_c.send(os_id.clone()).unwrap();
                // Wait for threads B and C to confirm they have created their instances.
                rx_a.recv().unwrap();
                rx_a.recv().unwrap();
                os_id
                // Owned shmem drops here after a second owned instance has been
                // dropped in thread B.
            };
            // Should not be able to reopen shared memory after an owned instance
            // has been dropped in thread B.
            // assert!(ShmemConf::new().size(4096).os_id(os_id).open().is_err());
            // Tell thread C to drop the unowned instance.
            tx_c.send(String::new()).unwrap();
        })
        .unwrap();
    let thread_b = thread::Builder::new()
        .name(String::from("B"))
        .spawn({
            let tx_a = tx_a.clone();
            move || {
                let existing_os_id = rx_b.recv().unwrap();
                // Creating two `Shmem`s with the same `os_id` should fail
                assert!(ShmemConf::new()
                    .size(4096)
                    .os_id(&existing_os_id)
                    .create()
                    .is_err());
                {
                    // Should be able to open the existing shared memory
                    let mut shmem = ShmemConf::new().os_id(&existing_os_id).open().unwrap();
                    shmem.set_owner(true);
                    tx_a.send(String::new()).unwrap();
                    // When the owning shmem is dropped here, we
                    // 1. should be able to still drop the original shared memory in thread A.
                    // 2. should not be able to reopen it with the same name in thread A, even
                    // if an instance is kept alive in thread C.
                }
            }
        })
        .unwrap();
    let thread_c = thread::Builder::new()
        .name(String::from("C"))
        .spawn(move || {
            // This thread keeps a shared memory instance alive until it's told to
            // drop it.
            let existing_os_id = rx_c.recv().unwrap();
            let _shmem = ShmemConf::new().os_id(&existing_os_id).open().unwrap();
            // Indicate to thread A that the instance has been created.
            tx_a.send(String::new()).unwrap();
            // Shut down signal.
            rx_c.recv().unwrap();
        })
        .unwrap();
    thread_a.join().unwrap();
    thread_b.join().unwrap();
    thread_c.join().unwrap();
}
