use shared_memory::ShmemConf;

#[test]
fn main() {
    let os_id = {
        let mut shmem = ShmemConf::new().size(4096).create().unwrap();
        shmem.set_owner(false);
        String::from(shmem.get_os_id())
    };
    let _shmem = ShmemConf::new().os_id(os_id).open().unwrap();
}
