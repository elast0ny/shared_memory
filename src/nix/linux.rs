//This file contains the linux specific implementations
use super::{std,
    EventImpl,
    EventState,
    Timeout,
    GenericEvent,
    RawFd,
    nix,
    Errno,
    Result,
    c_void,
    Duration,
    Instant
};

#[doc(hidden)]
pub struct EventFdData {
    pub ep_fd: RawFd,
    pub evt_fd: RawFd,
    pub evt_val: [u8; 8],
    pub epoll_event: nix::sys::epoll::EpollEvent,
}
///Auto event using Linux's eventfd implementation
///
///A file descriptor from an event must be actively shared between processes
///through a unix socket. This means that a child process openning a shared memory mapping with
///an eventfd must connect to a socket created by the owner of the shmem and the creator
///must send the file descriptor.
pub struct AutoEventFd {}
impl EventImpl for AutoEventFd {
    ///Returns the size of the event structure that will live in shared memory
    fn size_of(&self) -> usize {
        //Eventfd cannot be shared through memory
        0
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<()> {
        //Allocate some data required to manage the eventfd
        let mut evt_data = Box::new(EventFdData{
            ep_fd: -1,
            evt_fd: -1,
            evt_val: [0; 8],
            epoll_event: nix::sys::epoll::EpollEvent::new(nix::sys::epoll::EpollFlags::EPOLLIN, 0)
        });

        //If we open, we do not have the file descriptor for the eventfd yet...
        if !create_new {
            // This is safely free'ed through self.destroy()
            event_info.ptr = Box::into_raw(evt_data) as *mut c_void;
            return Ok(())
        }

        //Create epoll context
        evt_data.ep_fd = nix::sys::epoll::epoll_create()?;

        //Create the eventfd
        evt_data.evt_fd = nix::sys::eventfd::eventfd(0, nix::sys::eventfd::EfdFlags::EFD_NONBLOCK)?;

        //Add the eventfd to our epoll context
        nix::sys::epoll::epoll_ctl(evt_data.ep_fd, nix::sys::epoll::EpollOp::EpollCtlAdd, evt_data.evt_fd, Some(&mut evt_data.epoll_event))?;

        // This is safely free'ed through self.destroy()
        event_info.ptr = Box::into_raw(evt_data) as *mut c_void;

        Ok(())
    }
    fn destroy(&self, event_info: &mut GenericEvent) {
        if !event_info.ptr.is_null() {
            let my_mem = unsafe {Box::from_raw(event_info.ptr as *mut EventFdData)};
            drop(my_mem);
        }
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<()> {

        let my_data: &mut EventFdData = unsafe { &mut (*(event_ptr as *mut EventFdData))};

        //Convert our timeout into milliseconds for epoll_wait
        let timeout_duration: Duration;
        let timeout_ms = match timeout {
            Timeout::Infinite => -1,
            Timeout::Sec(t) => (t * 1000) as isize,
            Timeout::Milli(t) => (t) as isize,
            Timeout::Micro(t) => (t / 1000) as isize,
            Timeout::Nano(t) => (t / 1000000) as isize,
        };
        timeout_duration = Duration::from_millis(timeout_ms as u64);


        //Loop until we either got the event or timeout is hit
        let start_time = Instant::now();
        loop {
            //Wait for the FD to be ready
            let res = nix::sys::epoll::epoll_wait(my_data.ep_fd, &mut [my_data.epoll_event], timeout_ms)?;
            if res != 1 {
                return Err(From::from("Timeout"));
            }

            //Consume the event
            match nix::unistd::read(my_data.evt_fd, &mut my_data.evt_val) {
                Ok(_v) => {
                    //We got our event !
                    break;
                },
                Err(nix::Error::Sys(Errno::EAGAIN)) => {
                    //This would happen if someone read the eventfd between our epoll_wait and read calls
                    if timeout_ms != -1 && start_time.elapsed() >= timeout_duration {
                        return Err(From::from("Timeout"));
                    } else {
                        continue;
                    }
                },
                Err(e) => return Err(From::from(e)),
            };
        }

        Ok(())
    }
    ///This method sets the event state
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<()> {

        let my_data: &mut EventFdData = unsafe { &mut (*(event_ptr as *mut EventFdData))};
        //write 8 bytes to the fd
        match state {
            EventState::Wait => {
                //Consume the event
                match nix::unistd::read(my_data.evt_fd, &mut my_data.evt_val) {
                    Ok(_v) => {},
                    Err(nix::Error::Sys(Errno::EAGAIN)) => {},
                    Err(e) => return Err(From::from(e)),
                };
            },
            EventState::Signaled => {
                nix::unistd::write(my_data.evt_fd, &mut unsafe {std::mem::transmute::<u64, [u8; 8]>(1)})?;
            },
        }
        Ok(())
    }
}

pub struct ManualEventFd {}
impl EventImpl for ManualEventFd {
    ///Returns the size of the event structure that will live in shared memory
    fn size_of(&self) -> usize {
        //Eventfd cannot be shared through memory
        0
    }
    ///Initializes the event
    fn init(&self, event_info: &mut GenericEvent, create_new: bool) -> Result<()> {
        //Allocate some data required to manage the eventfd
        let mut evt_data = Box::new(EventFdData{
            ep_fd: nix::sys::epoll::epoll_create()?,
            evt_fd: -1,
            evt_val: [0; 8],
            epoll_event: nix::sys::epoll::EpollEvent::new(nix::sys::epoll::EpollFlags::EPOLLIN, 0)
        });

        //If we open, we do not have the file descriptor for the eventfd yet...
        if !create_new {
            // This is safely free'ed through self.destroy()
            event_info.ptr = Box::into_raw(evt_data) as *mut c_void;
            return Ok(())
        }

        //Create the eventfd
        evt_data.evt_fd = nix::sys::eventfd::eventfd(0, nix::sys::eventfd::EfdFlags::EFD_NONBLOCK)?;

        //Add the eventfd to our epoll context
        nix::sys::epoll::epoll_ctl(evt_data.ep_fd, nix::sys::epoll::EpollOp::EpollCtlAdd, evt_data.evt_fd, Some(&mut evt_data.epoll_event))?;

        // This is safely free'ed through self.destroy()
        event_info.ptr = Box::into_raw(evt_data) as *mut c_void;

        Ok(())
    }
    fn destroy(&self, event_info: &mut GenericEvent) {
        if !event_info.ptr.is_null() {
            let my_mem = unsafe {Box::from_raw(event_info.ptr as *mut EventFdData)};
            drop(my_mem);
        }
    }
    ///This method should only return once the event is signaled
    fn wait(&self, event_ptr: *mut c_void, timeout: Timeout) -> Result<()> {

        let my_data: &mut EventFdData = unsafe { &mut (*(event_ptr as *mut EventFdData))};

        //Convert our timeout into milliseconds for epoll_wait
        let timeout_ms = match timeout {
            Timeout::Infinite => -1,
            Timeout::Sec(t) => (t * 1000) as isize,
            Timeout::Milli(t) => (t) as isize,
            Timeout::Micro(t) => (t / 1000) as isize,
            Timeout::Nano(t) => (t / 1000000) as isize,
        };

        //Wait for the FD to be ready
        let res = nix::sys::epoll::epoll_wait(my_data.ep_fd, &mut [my_data.epoll_event], timeout_ms)?;
        if res != 1 {
            return Err(From::from("Timeout"));
        }

        //Do not consume the event

        Ok(())
    }
    ///This method sets the event state
    fn set(&self, event_ptr: *mut c_void, state: EventState) -> Result<()> {
        let my_data: &mut EventFdData = unsafe { &mut (*(event_ptr as *mut EventFdData))};
        match state {
            EventState::Wait => {
                //Consume the event
                match nix::unistd::read(my_data.evt_fd, &mut my_data.evt_val) {
                    Ok(_v) => {},
                    Err(nix::Error::Sys(Errno::EAGAIN)) => {},
                    Err(e) => return Err(From::from(e)),
                };
            },
            EventState::Signaled => {
                //TODO : There is a slight chance that we could overflow the u64...
                //      We could consume the event before setting it but that doubles
                //      the syscall overhead...
                nix::unistd::write(my_data.evt_fd, &mut unsafe {std::mem::transmute::<u64, [u8; 8]>(1)})?;
            },
        }
        Ok(())
    }
}
