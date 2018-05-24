use super::*;

///Raw shared memory mapping
///
/// This feature is only useful when dealing with memory mappings not managed by this crate.
/// When all processes involed use the shared_memory crate, it is highly recommended to avoid
/// SharedMemRaw and use the much safer/full-featured SharedMem.
pub struct SharedMemRaw {
    //Os specific data for the mapping
    os_data: os_impl::MapData,
}
impl SharedMemRaw {

    pub fn create(unique_id: String, size: usize) -> Result<SharedMemRaw> {

        let os_map: os_impl::MapData = os_impl::create_mapping(&unique_id, size)?;

        Ok(SharedMemRaw {
            os_data: os_map,
        })
    }
    pub fn open(unique_id: String) -> Result<SharedMemRaw> {

        //Attempt to open the mapping
        let os_map = os_impl::open_mapping(&unique_id)?;

        Ok(SharedMemRaw {
            os_data: os_map,
        })
    }

    ///Returns the size of the SharedMemRaw mapping
    pub fn get_size(&self) -> &usize {
        &self.os_data.map_size
    }
    ///Returns the OS specific path of the shared memory object
    ///
    /// Usualy on Linux, this will point to a "file" under /dev/shm/
    ///
    /// On Windows, this returns a namespace
    pub fn get_path(&self) -> &String {
        &self.os_data.unique_id
    }

    pub fn get_ptr(&self) -> *mut c_void {
        return self.os_data.map_ptr;
    }
}

impl ReadRaw for SharedMemRaw {
    unsafe fn get_raw<D: SharedMemCast>(&self) -> &D {
        return &(*(self.os_data.map_ptr as *const D))
    }

    unsafe fn get_raw_slice<D: SharedMemCast>(&self) -> &[D] {
        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size > self.os_data.map_size {
            panic!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size,  self.os_data.map_size);
        }
        let num_items: usize =  self.os_data.map_size / item_size;

        return slice::from_raw_parts(self.os_data.map_ptr as *const D, num_items);
    }
}
impl WriteRaw for SharedMemRaw {
    unsafe fn get_raw_mut<D: SharedMemCast>(&mut self) -> &mut D {
        return &mut (*(self.os_data.map_ptr as *mut D))
    }
    unsafe fn get_raw_slice_mut<D: SharedMemCast>(&mut self) -> &mut[D] {
        //Make sure that we can cast our memory to the slice
        let item_size = std::mem::size_of::<D>();
        if item_size >  self.os_data.map_size {
            panic!("Tried to map type of {} bytes to a lock holding only {} bytes", item_size,  self.os_data.map_size);
        }
        let num_items: usize =  self.os_data.map_size / item_size;

        return slice::from_raw_parts_mut(self.os_data.map_ptr as *mut D, num_items);
    }
}
