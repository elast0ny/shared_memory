use ::quick_error::quick_error;
use std::error::Error;

quick_error! {
    #[derive(Debug)]
    pub enum SharedMemError {
        RangeDoesNotFit(request: usize, available: usize) {
            description("The requested range does not fit")
            display(x) -> ("{} : (Requested : {}, Available : {}", x.description(), request, available)
        }
        RangeOverlapsExisting(offset: usize, length: usize, existing_lock_id: usize) {
            description("The requested range overlaps an existing lock range")
            display(x) -> ("{} : (Offset : {}, length : {}, overlaps with lock #{}", x.description(), offset, length, existing_lock_id)
        }
        MapSizeZero {
            description("You cannot create a shared memory mapping of 0 size")
        }
        LinkCreateFailed(err: std::io::Error) {
            description("Creating the link file failed")
            display(x) -> ("{} : {}", x.description(), err)
            cause(err)
        }
        LinkWriteFailed(err: std::io::Error) {
            description("Writing the link file failed")
            display(x) -> ("{} : {}", x.description(), err)
            cause(err)
        }
        LinkExists {
            description("Shared memory link already exists")
        }
        LinkOpenFailed(err: std::io::Error) {
            description("Openning the link file failed")
            display(x) -> ("{} : {}", x.description(), err)
            cause(err)
        }
        LinkReadFailed(err: std::io::Error) {
            description("Reading the link file failed")
            display(x) -> ("{} : {}", x.description(), err)
            cause(err)
        }
        LinkDoesNotExist {
            description("Requested link file does not exist")
        }
        InvalidHeader {
            description("Shared memory header is corrupt")
        }
        Timeout {
            description("Operation timed out")
        }
        MappingIdExists {
            description("Shared memory OS specific ID already exists")
        }
        MapCreateFailed(err: u32) {
            description("Creating the shared memory failed")
            display(x) -> ("{} : os error {}", x.description(), err)
        }
        MapOpenFailed(err: u32) {
            description("Openning the shared memory failed")
            display(x) -> ("{} : os error {}", x.description(), err)
        }
        UnknownOsError(err: u32) {
            description("An unexpected OS error occured")
            display(x) -> ("{} : os error {}", x.description(), err)
        }
        FailedToAcquireLock(err: u32) {
            description("Failed to acquire lock")
            display(x) -> ("{} : os error {}", x.description(), err)
        }
        FailedToCreateLock(err: u32) {
            description("Failed to create lock")
            display(x) -> ("{} : os error {}", x.description(), err)
        }
        FailedToSignalEvent(err: u32) {
            description("Failed to signal event")
            display(x) -> ("{} : os error {}", x.description(), err)
        }
        FailedToCreateEvent(err: u32) {
            description("Failed to create event")
            display(x) -> ("{} : os error {}", x.description(), err)
        }
    }
}
