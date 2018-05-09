use super::*;

pub struct GenericEvent {
    /* Fields shared in the memory mapping */
    pub uid: u8,
    /* Internal fields */
    pub ptr: *mut c_void,
    //TODO : pub interface: &'a SharedMemEventImpl,
}
