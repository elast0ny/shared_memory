use ::cfg_if::*;
use ::enum_primitive::*;

use std::mem::size_of;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::{SharedMemError, Timeout};

#[doc(hidden)]
pub struct GenericEvent {
    pub uid: u8,
    pub ptr: *mut c_void,
    pub interface: &'static dyn EventImpl,
}
impl Drop for GenericEvent {
    fn drop(&mut self) {
        self.interface.destroy(self);
    }
}

///Possible states for an event
pub enum EventState {
    ///An event set to "Wait" will cause subsequent wait() calls to block
    ///
    ///This is mostly usefull for manual events as auto events automatically reset to "Wait".
    Wait,
    ///An event set to "Signaled" will unblock threads who are blocks on wait() calls.
    ///
    ///If this is an Auto lock, only one waiting thread will be unblocked as
    ///the state will be automatically set to WAIT after the first threads wakes up.
    Signaled,
}

//TODO : This is super ugly, not sure how to fix though...
cfg_if! {
    if #[cfg(target_os="linux")] {
        enum_from_primitive! {
            #[derive(Debug,Copy,Clone)]
            ///List of available signaling mechanisms on your platform.
            pub enum EventType {
                ///Busy event that automatically resets after a wait
                AutoBusy = 0,
                ///Busy event that needs to be reset manually
                ManualBusy,
                ///Generic event that automatically resets after a wait
                Auto,
                ///Generic event that needs to be reset manually
                Manual,
                ///Linux eventfd event that automatically resets after a wait
                AutoEventFd,
                ///Linux eventfd event that needs to be reset manually
                ManualEventFd,
            }
        }
    } else {
        enum_from_primitive! {
            #[derive(Debug,Copy,Clone)]
            ///List of available signaling mechanisms on your platform.
            pub enum EventType {
                ///Busy event that automatically resets after a wait
                AutoBusy = 0,
                ///Busy event that needs to be reset manually
                ManualBusy,
                ///Generic event that automatically resets after a wait
                Auto,
                ///Generic event that needs to be reset manually
                Manual,
            }
        }
    }
}

///All events implement this trait
#[doc(hidden)]
pub trait EventImpl {
    ///Returns the size of the event structure that will live in shared memory
    fn size_of(&self) -> usize;
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<(), SharedMemError>;
    ///De-initializes the event
    fn destroy(&self, event_info: &mut GenericEvent);
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<(), SharedMemError>;
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<(), SharedMemError>;
}

///Provides the ability to set an event to a state
pub trait EventSet {
    ///Set an event to a specific state
    ///
    ///The caller must validate event_index before calling this method
    fn set(&mut self, event_index: usize, state: EventState) -> Result<(), SharedMemError>;
}

///Provides the ability to wait on an event
pub trait EventWait {
    ///Wait for an event to become signaled or until timeout is reached
    ///
    ///The caller must validate event_index before calling this method
    fn wait(&self, event_index: usize, timeout: Timeout) -> Result<(), SharedMemError>;
}

/* Cross platform Event Implementation */

fn timeout_to_duration(timeout: Timeout) -> Duration {
    Duration::from_millis(match timeout {
        Timeout::Infinite => !(0 as u64),
        Timeout::Sec(t) => (t * 1000) as u64,
        Timeout::Milli(t) => (t) as u64,
        Timeout::Micro(t) => (t / 1000) as u64,
        Timeout::Nano(t) => (t / 1_000_000) as u64,
    })
}

#[doc(hidden)]
pub struct AutoBusy {}
impl EventImpl for AutoBusy {
    fn size_of(&self) -> usize {
        size_of::<AtomicBool>()
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<(), SharedMemError> {
        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        let signal: &AtomicBool = unsafe { &mut (*(event_info.ptr as *mut AtomicBool)) };
        signal.store(false, Ordering::Relaxed);

        Ok(())
    }
    ///De-initializes the event
    fn destroy(&self, _event_info: &mut GenericEvent) {
        //Nothing to do here
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<(), SharedMemError> {
        let signal: &AtomicBool = unsafe { &mut (*(event_ptr as *mut AtomicBool)) };

        let timeout_len: Duration = match timeout {
            Timeout::Infinite => {
                while signal
                    .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
                    .is_err()
                {}
                return Ok(());
            }
            _ => timeout_to_duration(timeout),
        };

        //let check_interval = 5;
        //let mut num_attemps: usize = 0;
        let start_time: Instant = Instant::now();

        //Busy loop checking timeout every 5 iterations
        while signal
            .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            //num_attemps = num_attemps.wrapping_add(1);
            //if num_attemps%check_interval == 0 {
            if start_time.elapsed() >= timeout_len {
                return Err(SharedMemError::Timeout);
            }
            //}
        }

        Ok(())
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<(), SharedMemError> {
        let signal: &AtomicBool = unsafe { &mut (*(event_ptr as *mut AtomicBool)) };

        signal.store(
            match state {
                EventState::Wait => false,
                EventState::Signaled => true,
            },
            Ordering::Relaxed,
        );

        Ok(())
    }
}

#[doc(hidden)]
pub struct ManualBusy {}
impl EventImpl for ManualBusy {
    fn size_of(&self) -> usize {
        size_of::<AtomicBool>()
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<(), SharedMemError> {
        //Nothing to do if we're not the creator
        if !create_new {
            return Ok(());
        }

        let signal: &AtomicBool = unsafe { &mut (*(event_info.ptr as *mut AtomicBool)) };
        signal.store(false, Ordering::Relaxed);

        Ok(())
    }
    ///De-initializes the event
    fn destroy(&self, _event_info: &mut GenericEvent) {
        //Nothing to do here
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<(), SharedMemError> {
        let signal: &AtomicBool = unsafe { &mut (*(event_ptr as *mut AtomicBool)) };

        let timeout_len: Duration = match timeout {
            Timeout::Infinite => {
                while !signal.load(Ordering::Relaxed) {}
                return Ok(());
            }
            _ => timeout_to_duration(timeout),
        };

        //let check_interval = 5;
        //let mut num_attemps: usize = 0;
        let start_time: Instant = Instant::now();

        //Busy loop checking timeout every 5 iterations
        while !signal.load(Ordering::Relaxed) {
            //num_attemps = num_attemps.wrapping_add(1);
            //if num_attemps%check_interval == 0 {
            if start_time.elapsed() >= timeout_len {
                return Err(SharedMemError::Timeout);
            }
            //}
        }
        Ok(())
    }
    ///This method sets the event. This should never block
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<(), SharedMemError> {
        let signal: &AtomicBool = unsafe { &mut (*(event_ptr as *mut AtomicBool)) };

        signal.store(
            match state {
                EventState::Wait => false,
                EventState::Signaled => true,
            },
            Ordering::Relaxed,
        );

        Ok(())
    }
}
