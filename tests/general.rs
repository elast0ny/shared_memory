use std::path::Path;

use shared_memory::ShmemConf;

#[test]
fn create_new() {
    let mut s = ShmemConf::new().size(4090).create().unwrap();

    assert!(s.is_owner());
    assert!(!s.get_os_id().is_empty());
    assert!(s.len() >= 4090);
    assert!(!s.as_ptr().is_null());
    unsafe {
        assert_eq!(s.as_slice().len(), s.len());
        assert_eq!(s.as_slice_mut().len(), s.len());
    }
}

#[test]
fn create_with_flink() {
    let flink = Path::new("create_new1");

    let mut s = ShmemConf::new().flink(flink).size(4090).create().unwrap();

    assert!(s.is_owner());
    assert!(!s.get_os_id().is_empty());
    assert!(flink.is_file());
    assert!(s.len() >= 4090);
    assert!(!s.as_ptr().is_null());
    unsafe {
        assert_eq!(s.as_slice().len(), s.len());
        assert_eq!(s.as_slice_mut().len(), s.len());
    }

    drop(s);

    assert!(!flink.is_file());
}

#[test]
fn open_os_id() {
    let s1 = ShmemConf::new().size(4090).create().unwrap();

    // Open with the unique os id
    let os_id = s1.get_os_id().to_string();
    let mut s2 = ShmemConf::new().os_id(&os_id).open().unwrap();

    assert!(!s2.is_owner());
    assert!(!s2.get_os_id().is_empty());
    assert!(s2.len() >= 4090);
    assert!(!s2.as_ptr().is_null());
    unsafe {
        assert_eq!(s2.as_slice().len(), s2.len());
        assert_eq!(s2.as_slice_mut().len(), s2.len());
    }

    // Drop the owner of the mapping
    drop(s1);

    // Make sure it can be openned again
    assert!(ShmemConf::new().os_id(&os_id).open().is_err());

    drop(s2);
}

#[test]
fn open_flink() {
    let flink = Path::new("create_new2");
    let s1 = ShmemConf::new().flink(flink).size(4090).create().unwrap();

    // Open with file base link
    let mut s2 = ShmemConf::new().flink(flink).open().unwrap();

    assert!(!s2.is_owner());
    assert!(!s2.get_os_id().is_empty());
    assert!(flink.is_file());
    assert!(s2.len() >= 4090);
    assert!(!s2.as_ptr().is_null());
    unsafe {
        assert_eq!(s2.as_slice().len(), s2.len());
        assert_eq!(s2.as_slice_mut().len(), s2.len());
    }

    // Drop the owner of the mapping
    drop(s1);

    // Make sure it can be openned again
    assert!(ShmemConf::new().flink(flink).open().is_err());

    drop(s2);
}

#[test]
fn share_data() {
    let s1 = ShmemConf::new()
        .size(core::mem::size_of::<u32>())
        .create()
        .unwrap();

    // Open with the unique os id
    let os_id = s1.get_os_id().to_string();
    let s2 = ShmemConf::new().os_id(os_id).open().unwrap();

    let ptr1 = s1.as_ptr() as *mut u32;
    let ptr2 = s2.as_ptr() as *mut u32;

    // Confirm that the two pointers are different
    assert_ne!(ptr1, ptr2);

    // Write a value from s1 and read it from s2
    unsafe {
        let shared_val = 0xBADC0FEE;
        ptr1.write_volatile(shared_val);
        let read_val = ptr2.read_volatile();
        assert_eq!(read_val, shared_val);
    }
}
