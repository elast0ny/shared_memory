use super::*;

#[doc(hidden)]
pub struct GenericEvent<'a> {
    pub uid: u8,
    pub ptr: *mut c_void,
    pub interface: &'a EventImpl,
}
impl<'a> Drop for GenericEvent<'a> {
    fn drop(&mut self) {
        self.interface.destroy(self);
    }
}

///States that events can be set to
pub enum EventState {
    ///An event set to WAIT will cause subsequent wait() calls to block
    ///
    ///This is mostly usefull for Manual* events.
    Wait,
    ///An event set to SIGNALED will unblock blocked wait() calls.
    ///
    ///If this is an Auto lock, only one waiting thread will be unblocked as
    ///the state will be automatically set to WAIT after the unblock.
    Signaled,
}

enum_from_primitive! {
    #[derive(Debug,Copy,Clone)]
    ///Different types of events one can add to a SharedMemConf
    pub enum EventType {
        ///Busy event that automatically resets after a wait
        AutoBusy = 0,
        ///Busy event that needs to be reset manually
        ManualBusy,
        ///Generic event that automatically resets after a wait
        Auto,
        ///Generic event that needs to be reset manually
        Manual,
        #[cfg(target_os="linux")]
        ///Linux eventfd event that automatically resets after a wait
        AutoEventFd,
        #[cfg(target_os="linux")]
        ///Linux eventfd event that needs to be reset manually
        ManualEventFd,
    }
}

///All events implement this trait
#[doc(hidden)]
pub trait EventImpl {
    ///Returns the size of the event structure that will live in shared memory
    fn size_of(&self) -> usize;
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<()>;
    ///De-initializes the event
    fn destroy(&self, event_info: &mut GenericEvent);
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<()>;
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<()>;
}

///Structs implementing this trait allows users to signal events
pub trait EventSet {
    fn set(&mut self, event_index: usize, state: EventState) -> Result<()>;
}

///Structs implementing this trait allows users to wait on events
pub trait EventWait {
    fn wait(&self, event_index: usize, timeout: Timeout) -> Result<()>;
}
