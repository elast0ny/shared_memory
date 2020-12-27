use ::quick_error::quick_error;

quick_error! {
    #[derive(Debug)]
    pub enum ShmemError {
        MapSizeZero {
            display("You cannot create a shared memory mapping of 0 size")
        }
        NoLinkOrOsId {
            display("Tried to open mapping without flink path or os_id")
        }
        FlinkInvalidOsId {
            display("Tried to open mapping from both flink and os_id but the flink did not point to the same os_id")
        }
        LinkCreateFailed(err: std::io::Error) {
            display(x) -> ("Creating the link file failed, {}", err)
            source(err)
        }
        LinkWriteFailed(err: std::io::Error) {
            display(x) -> ("Writing the link file failed, {}", err)
            source(err)
        }
        LinkExists {
            display("Shared memory link already exists")
        }
        LinkOpenFailed(err: std::io::Error) {
            display(x) -> ("Openning the link file failed, {}", err)
            source(err)
        }
        LinkReadFailed(err: std::io::Error) {
            display(x) -> ("Reading the link file failed, {}", err)
            source(err)
        }
        LinkDoesNotExist {
            display("Requested link file does not exist")
        }
        MappingIdExists {
            display("Shared memory OS specific ID already exists")
        }
        MapCreateFailed(err: u32) {
            display(x) -> ("Creating the shared memory failed, os error {}", err)
        }
        MapOpenFailed(err: u32) {
            display(x) -> ("Openning the shared memory failed, os error {}", err)
        }
        UnknownOsError(err: u32) {
            display(x) -> ("An unexpected OS error occured, os error {}", err)
        }
    }
}