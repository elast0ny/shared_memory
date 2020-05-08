use ::quick_error::quick_error;

quick_error! {
    #[derive(Debug)]
    pub enum ShmemError {
        MapSizeZero {
            description("You cannot create a shared memory mapping of 0 size")
        }
        NoLinkOrOsId {
            description("Tried to open mapping without flink path or os_id")
        }
        FlinkInvalidOsId {
            description("Tried to open mapping from both flink and os_id but the flink did not point to the same os_id")
        }
        LinkCreateFailed(err: std::io::Error) {
            description("Creating the link file failed")
            display(x) -> ("{} : {}", x, err)
            cause(err)
        }
        LinkWriteFailed(err: std::io::Error) {
            description("Writing the link file failed")
            display(x) -> ("{} : {}", x, err)
            cause(err)
        }
        LinkExists {
            description("Shared memory link already exists")
        }
        LinkOpenFailed(err: std::io::Error) {
            description("Openning the link file failed")
            display(x) -> ("{} : {}", x, err)
            cause(err)
        }
        LinkReadFailed(err: std::io::Error) {
            description("Reading the link file failed")
            display(x) -> ("{} : {}", x, err)
            cause(err)
        }
        LinkDoesNotExist {
            description("Requested link file does not exist")
        }
        MappingIdExists {
            description("Shared memory OS specific ID already exists")
        }
        MapCreateFailed(err: u32) {
            description("Creating the shared memory failed")
            display(x) -> ("{} : os error {}", x, err)
        }
        MapOpenFailed(err: u32) {
            description("Openning the shared memory failed")
            display(x) -> ("{} : os error {}", x, err)
        }
        UnknownOsError(err: u32) {
            description("An unexpected OS error occured")
            display(x) -> ("{} : os error {}", x, err)
        }
    }
}
