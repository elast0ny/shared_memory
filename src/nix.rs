use std;

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

pub struct MemPermission {

}

pub struct MemMetadata {

}

pub struct MemFile {

}

impl MemFile {
    pub fn open(path: &std::path::Path) -> Result<MemFile> {
        Ok(MemFile {})
    }
    pub fn create(path: &std::path::Path) -> Result<MemFile> {
        Ok(MemFile {})
    }

    pub fn set_len(&self, size: u64) -> Result<()> {
        Err(From::from("unimplemented !"))
    }

    pub fn metadata(&self) -> Result<MemMetadata> {
        Err(From::from("unimplemented !"))
    }

    pub fn set_permissions(&self, perm: MemPermission) -> Result<()> {
        Err(From::from("unimplemented !"))
    }
}


impl std::io::Read for MemFile {

}

impl std::io::Write for MemFile {

}

impl std::io::Seek for MemFile {

}
