//! This crate provides the implementation of `#[derive(SharedMemCast)]` for the
//! [`shared_memory` crate](https://crates.io/crates/shared_memory).
//!
//! You should never have to use this crate directly. All of its functionality is
//! re-exported from the `shared_memory` crate.

extern crate proc_macro;

use proc_macro::TokenStream;

#[proc_macro_derive(SharedMemCast)]
pub fn derive(input: TokenStream) -> TokenStream {
    let _ = input;

    unimplemented!()
}
