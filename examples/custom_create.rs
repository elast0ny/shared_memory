extern crate shared_memory;
use shared_memory::*;

#[macro_use]
extern crate enum_primitive;
pub use enum_primitive::FromPrimitive;

enum_from_primitive! {
    enum MyEvents {
        MyEvt = 0,
        PeerEvt,
    }
}

#[inline]
//Converts enum to usize index
fn ind(ev_type: MyEvents) -> usize {
    ev_type as usize
}

fn main() -> Result<(), SharedMemError> {
    //Create a custom configuration for our mapping
    let mut my_conf = SharedMemConf::default()
        .set_link_path("shared_mem.link")
        .set_os_path("test_mapping")
        .set_size(4096);

    //Add an event for every variation of our MyEvents enum
    let mut i: u8 = 0;
    while let Some(_v) = MyEvents::from_u8(i) {
        my_conf = my_conf.add_event(EventType::Auto)?;
        i += 1;
    }

    //Create mapping based on our config
    let mut my_shmem = my_conf.create()?;

    println!("Created link file with info : {}", my_shmem);

    //Simulate some signaling
    println!("Waiting for peer to signal for 5s");
    match my_shmem.wait(ind(MyEvents::PeerEvt), Timeout::Sec(5)) {
        Ok(()) => println!("\tGot signal !"),
        Err(_) => println!("\tNo signal :("),
    };

    println!("Signaling peer...");
    my_shmem.set(ind(MyEvents::MyEvt), EventState::Signaled)?;

    println!("Sleeping for 5s");
    std::thread::sleep(std::time::Duration::from_secs(5));

    println!("Signaling peer again...");
    my_shmem.set(ind(MyEvents::MyEvt), EventState::Signaled)?;

    println!("Waiting for peer to signal for 10s");
    match my_shmem.wait(ind(MyEvents::PeerEvt), Timeout::Sec(10)) {
        Ok(()) => println!("\tGot signal !"),
        Err(_) => println!("\tNo signal :("),
    };

    println!("Done !");
    return Ok(());
}
