extern crate winapi;

use super::*;
use self::winapi::shared::ntdef::{NULL};
use self::winapi::shared::minwindef::{DWORD, LPVOID, FALSE};
use self::winapi::um::winbase::*;
use self::winapi::um::winnt::*;
use self::winapi::um::handleapi::*;
use self::winapi::um::memoryapi::*;
use self::winapi::um::errhandlingapi::*;
use std::slice;

use std::mem::size_of;
use std::path::PathBuf;
use std::ffi::CString;
use std::sync::*;
use std::ptr::{null_mut};

use std::fs::{File, remove_file};
use std::io::{Write, Read};

type Result<T> = std::result::Result<T, Box<std::error::Error>>;

pub struct MemMetadata {
    mem_lock: RwLock<*mut [u8]>,
}

pub struct MemFile {
    owner: bool,
    pub file_path: PathBuf,
    pub file_map: HANDLE,
    pub mem_perm: MemPermission,
    pub mem_size: u64,
    mem_meta: Option<*mut MemMetadata>,
    mem_addr:  Option<*mut [u8]>,
}

impl Drop for MemFile {
    fn drop(&mut self) {
        unsafe {
            //Unmap memory
            if self.mem_addr.is_some() {
                UnmapViewOfFile(self.mem_addr.unwrap() as LPVOID);
            }
            //Close mapping
            if self.file_map != INVALID_HANDLE_VALUE {
                CloseHandle(self.file_map);
            }
        }

        //Delete file on disk if we created it
        if self.owner && self.file_path.is_file() {
            match remove_file(&self.file_path) {_=>{},};
        }
    }
}

impl MemFile {
    pub fn open(path: &std::path::Path, perm: MemPermission) -> Result<MemFile> {

        if !perm.read {
            return Err(From::from("Cannot open mapping without read permissions"));
        }

        let mut cur_file: MemFile = MemFile {
            owner:false,
            file_path: PathBuf::from(path),
            file_map: INVALID_HANDLE_VALUE,
            mem_perm: perm,
            mem_size: 0,
            mem_meta: None,
            mem_addr: None};

        //Get namespace of shared memory
        let mut disk_file = File::open(&cur_file.file_path)?;
        let mut file_contents: Vec<u8> = Vec::with_capacity(cur_file.file_path.to_string_lossy().len() + 5);
        disk_file.read_to_end(&mut file_contents)?;
        let content_str: String = String::from_utf8(file_contents)?;
        let mut content_tokens: std::str::SplitWhitespace = content_str.split_whitespace();

        let map_ns_path: &str = match content_tokens.next() {
            Some(v) => v,
            None => return Err(From::from("Unable to find namespace of mapping in file...")),
        };

        //TODO : We should probably not trust the size in this file... Dont know if Windows has a standard way of getting the size of a mapping
        let size: u64 = match content_tokens.next() {
            Some(v) => v.parse::<u64>()?,
            None => return Err(From::from("Unable to find size of mapping in file...")),
        };

        let mut map_perms: DWORD = 0;
        if cur_file.mem_perm.read {
            map_perms |= FILE_MAP_READ;
        }
        if cur_file.mem_perm.write{
            map_perms |= FILE_MAP_WRITE;
        }
        if cur_file.mem_perm.execute{
            map_perms |= FILE_MAP_EXECUTE;
        }

        println!("File mapping is {} of size {}", map_ns_path, size);

        unsafe {
            cur_file.file_map = OpenFileMappingA(map_perms, FALSE, CString::new(map_ns_path)?.as_ptr());
            if cur_file.file_map == NULL {
                return Err(From::from(format!("CreateFileMappingA failed with {}", GetLastError())));
            }

            let ptr = MapViewOfFile(cur_file.file_map, map_perms, 0, 0, 0);
            if ptr == NULL {
                return Err(From::from(format!("MapViewOfFile failed with {}", GetLastError())));
            }

            cur_file.mem_addr = Some(slice::from_raw_parts_mut(ptr as *mut u8, size as usize));
            cur_file.mem_size = size;
        }

        Ok(cur_file)
    }

    //Creates a unique memory mapped file with set permission and size
    pub fn create(path: &std::path::Path, perm: MemPermission, mut size:  u64) -> Result<MemFile> {

        if !perm.read {
            return Err(From::from("Cannot create mapping without read permissions"));
        }

        let mut cur_file: MemFile = MemFile{
            owner:true,
            file_path: PathBuf::new(),
            file_map: INVALID_HANDLE_VALUE,
            mem_perm: perm,
            mem_size: 0,
            mem_meta: None,
            mem_addr: None};

        //Validate requested permissions
        let mut map_perms: DWORD = 0;
        if cur_file.mem_perm.read && cur_file.mem_perm.write && cur_file.mem_perm.execute {
            map_perms |= PAGE_EXECUTE_READWRITE;
        } else if cur_file.mem_perm.read & cur_file.mem_perm.execute {
            map_perms |= PAGE_EXECUTE_READ;
        } else if cur_file.mem_perm.read && cur_file.mem_perm.write {
            map_perms |= PAGE_READWRITE;
        } else if cur_file.mem_perm.read {
            map_perms |= PAGE_READONLY;
        }

        if path.is_file() {
            return Err(From::from("File already exists"));
        }

        let mut disk_file = File::create(&path)?;

        //Make sure we just created a file, not a dir (is this necessary ?)
        if !path.is_file() {
            std::fs::remove_file(&path)?;
            return Err(From::from("Path given isnt a file"));
        }

        //Sanitize filepath to use for namespace
        let mut tmp_path: PathBuf = PathBuf::new();
        {
            cur_file.file_path = path.canonicalize()?;
            let abs_path: String = String::from(cur_file.file_path.to_string_lossy());
            let mut sanitized_path: String = String::with_capacity(abs_path.len());

            for c in abs_path.chars() {
                match c {
                    '?' | ':' /*| '\\' | '.'*/ => continue,
                    '\\' => sanitized_path.push('_'),
                    v => sanitized_path.push(v),
                }
            }
            tmp_path.push(sanitized_path.trim_matches('_'));
        }
        let unique_ns: String = String::from(tmp_path.to_string_lossy());

        //Create mapping and map to our address space
        unsafe {
            let full_size: u64 = size + size_of::<MemMetadata>() as u64;
            let high_size: u32 = (full_size & 0xFFFFFFFF00000000 as u64) as u32;
            let low_size: u32 = (full_size & 0xFFFFFFFF as u64) as u32;

            cur_file.file_map = CreateFileMappingA(INVALID_HANDLE_VALUE, null_mut(), map_perms, high_size, low_size, CString::new(unique_ns.clone())?.as_ptr());

            if cur_file.file_map == NULL {
                return Err(From::from(format!("CreateFileMappingA failed with {}", GetLastError())));
            }

            map_perms = 0;
            if cur_file.mem_perm.read {
                map_perms |= FILE_MAP_READ;
            }
            if cur_file.mem_perm.write{
                map_perms |= FILE_MAP_WRITE;
            }
            if cur_file.mem_perm.execute{
                map_perms |= FILE_MAP_EXECUTE;
            }

            let ptr = MapViewOfFile(cur_file.file_map, map_perms, 0, 0, 0);
            if ptr == NULL {
                return Err(From::from(format!("MapViewOfFile failed with {}", GetLastError())));
            }

            let meta_start_addr = ptr as *mut u8;
            let data_start_addr = (ptr as usize + size_of::<MemMetadata>()) as *mut u8;

            {
                //Initialise a mock MemMetadata struct
                let meta_template = MemMetadata {mem_lock: RwLock::new(slice::from_raw_parts_mut(data_start_addr, size as usize))};
                let template_ptr = slice::from_raw_parts_mut(&meta_template as *const _ as *mut u8, size_of::<MemMetadata>());

                let meta_dst = slice::from_raw_parts_mut(meta_start_addr, size_of::<MemMetadata>());

                //Copy over the template into our shared memory
                for i in 0..size_of::<MemMetadata>() {
                    meta_dst[i] = template_ptr[i];
                }
            }

            /*
            println!("Meta : {:p}", meta_start_addr);
            println!("Data : {:p}", data_start_addr);
            */

            cur_file.mem_meta = Some(meta_start_addr as *mut _ as *mut MemMetadata);
            cur_file.mem_addr = Some(slice::from_raw_parts_mut(data_start_addr, size as usize));
            cur_file.mem_size = size;
        }

        //Write namespace and size of allocation
        let file_content: String = format!("{} {}", unique_ns, size);
        match disk_file.write(&file_content.as_bytes()) {
            Ok(write_sz) => if write_sz != file_content.as_bytes().len() {
                return Err(From::from("Failed to write full contents info on disk"));
            },
            Err(_) => return Err(From::from("Failed to write info on disk")),
        };


        println!("Getting memlock");
        {
            let meta_data: &MemMetadata = unsafe{ &mut (*(cur_file.mem_meta.unwrap()))};
            println!("Address of meta data {:p}", meta_data);
            let mut lock_guard = meta_data.mem_lock.read().unwrap();
            println!("Lock Guard holds : {:?}", lock_guard);
            let shmem: &mut [u8] = unsafe { &mut (**(lock_guard))};
            shmem[0] = 0x10;
            println!("Address of data      {:p} = {}", shmem.as_ptr(), shmem[0]);
            drop(cur_file);
            return Err(From::from("test"));
        }

        Ok(cur_file)
    }
}
/*
impl std::io::Write for MemFile {

}

impl std::io::Seek for MemFile {

}
*/
