#[macro_use]
extern crate cfg_if;

cfg_if! {
     if #[cfg(windows)] {
        pub mod win;
        pub use win::*;
    } else if #[cfg(nix)] {
        pub mod nix;
        pub use nix::*;
    } else {
        unimplemented!("This target is not yet implemented !");
    }
}

pub struct MemPermission {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}
