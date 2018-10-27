extern crate shared_memory;
use shared_memory::*;
use std::ffi::OsStr;

#[macro_use]
extern crate enum_primitive;
pub use enum_primitive::FromPrimitive;

enum_from_primitive! {
    enum MyEvents {
        //Opposite of create_advances.rs
        PeerEvt = 0,
        MyEvt,
    }
}

#[inline]
fn ind(ev_type: MyEvents) -> usize {
    ev_type as usize
}

fn main() {

    //Open an existing shared SharedMem
    let mut my_shmem = match SharedMem::open_linked(OsStr::new("shared_mem.link")) {
        Ok(v) => v,
        Err(e) => {
            println!("Error : {}", e);
            println!("Failed to open SharedMem...");
            return;
        }
    };

    //Count how many variants are in our MyEvents enum
    let mut expected_num_events: usize = 0;
    while let Some(_v) = MyEvents::from_usize(expected_num_events) {
        expected_num_events += 1;
    }
    //Make sure mapping contains all of our events
    if expected_num_events != my_shmem.num_events() {
        println!("We expected {} events but {} are in the shared memory...", expected_num_events, my_shmem.num_events());
        return;
    }

    println!("Openned link file with info : {}", my_shmem);

    //Simulate some signaling
    println!("Signaling peer...");
    match my_shmem.set(ind(MyEvents::MyEvt), EventState::Signaled) {
        Ok(_) => {},
        Err(e) => {
            println!("Failed to signal peer !");
            println!("{}", e);
        }
    };

    println!("Waiting for peer to signal for 10s");
    match my_shmem.wait(ind(MyEvents::PeerEvt), Timeout::Sec(10)) {
        Ok(()) => println!("\tGot event !"),
        Err(_) => println!("\tNo events :("),
    };

    println!("Waiting for peer to signal for 10s");
    match my_shmem.wait(ind(MyEvents::PeerEvt), Timeout::Sec(10)) {
        Ok(()) => println!("\tGot event !"),
        Err(_) => println!("\tNo events :("),
    };

    println!("Signaling peer...");
    match my_shmem.set(ind(MyEvents::MyEvt), EventState::Signaled) {
        Ok(_) => {},
        Err(e) => {
            println!("Failed to signal peer !");
            println!("{}", e);
        }
    };

    println!("Reseting peer event to wait in 1 second");
    std::thread::sleep(std::time::Duration::from_secs(1));
    match my_shmem.set(ind(MyEvents::MyEvt), EventState::Wait) {
        Ok(_) => {},
        Err(e) => {
            println!("Failed to signal peer !");
            println!("{}", e);
        }
    };

    println!("Done !");

    return;
}
