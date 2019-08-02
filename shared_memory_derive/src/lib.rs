//! This crate provides the implementation of `#[derive(SharedMemCast)]` for the
//! [`shared_memory` crate](https://crates.io/crates/shared_memory).
//!
//! You should never have to use this crate directly. All of its functionality is
//! re-exported from the `shared_memory` crate.

extern crate proc_macro;

use quote::{quote};
use syn::{parse_macro_input, parse_quote, DeriveInput, GenericParam, Generics};

#[proc_macro_derive(SharedMemCast)]
pub fn derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {

    // Much of this code is just an adapted version of the great tutorial from the syn crate:
    // https://github.com/dtolnay/syn/blob/master/examples/heapsize/heapsize_derive/src/lib.rs

    let mut input = parse_macro_input!(input as DeriveInput);

    // Add a bound `T: SharedMemCast` to every type parameter T.
    add_generic_bounds(&mut input.generics);
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let name = input.ident;
    let expanded = quote! {
        unsafe impl #impl_generics shared_memory::SharedMemCast for #name #ty_generics
            #where_clause {}
    };

    expanded.into()
}

/// Add a bound `T: SharedMemCast` to every type parameter T.
fn add_generic_bounds(generics: &mut Generics) {
    for param in &mut generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            type_param.bounds.push(parse_quote!(shared_memory::SharedMemCast));
        }
    }
}
