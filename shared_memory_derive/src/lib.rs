//! This crate provides the implementation of `#[derive(SharedMemCast)]` for the
//! [`shared_memory` crate](https://crates.io/crates/shared_memory).
//!
//! You should never have to use this crate directly. All of its functionality is
//! re-exported from the `shared_memory` crate.

extern crate proc_macro;

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{
    parse_macro_input, parse_quote, spanned::Spanned, Data, DeriveInput, Fields, GenericParam,
    Generics, Ident, Type,
};

#[proc_macro_derive(SharedMemCast)]
pub fn derive(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    // The idea here is to generate code very similar to what the Eq trait generates. Eq is very
    // similar to the SharedMemCast trait in that Eq is also a marker trait and it requires that
    // all fields satisfy T: Eq.
    //
    // There is a secret struct std::cmp::AssertParamIsEq<T: Eq + ?Sized> that does the heavy
    // lifting. You can see it in the source code here:
    // https://github.com/rust-lang/rust/blob/c43753f910aae000f8bcb0a502407ea332afc74b/src/libcore/cmp.rs#L259-L268
    //
    // The code that #[derive(Eq)] generates for a simple struct Foo is as follows:
    //
    // struct Foo {
    //     x: i32,
    //     y: Bar,
    // }
    //
    // #[automatically_derived]
    // #[allow(unused_qualifications)]
    // impl ::std::cmp::Eq for Foo {
    //     #[inline]
    //     #[doc(hidden)]
    //     fn assert_receiver_is_total_eq(&self) -> () {
    //         {
    //             let _: ::std::cmp::AssertParamIsEq<i32>;
    //             let _: ::std::cmp::AssertParamIsEq<Bar>;
    //         }
    //     }
    // }
    //
    // Notice how this uses AssertParamIsEq to test each parameter without actually even
    // initializing anything. This uses a secret assert_receiver_is_total_eq method in the trait.
    // We will attempt to use the same technique.

    // Much of this code is just an adapted version of the great tutorial from the syn crate:
    // https://github.com/dtolnay/syn/blob/master/examples/heapsize/heapsize_derive/src/lib.rs

    let mut input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    // Add a bound `T: SharedMemCast` to every type parameter T.
    add_generic_bounds(&mut input.generics);
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let type_assertions = field_type_assertions(&name, &input.data);

    let expanded = quote! {
        unsafe impl #impl_generics shared_memory::SharedMemCast for #name #ty_generics #where_clause {
            fn assert_receiver_is_shared_mem_cast(&self) {
                #type_assertions
            }
        }
    };

    expanded.into()
}

/// Add a bound `T: SharedMemCast` to every type parameter T.
fn add_generic_bounds(generics: &mut Generics) {
    for param in &mut generics.params {
        if let GenericParam::Type(type_param) = param {
            type_param
                .bounds
                .push(parse_quote!(shared_memory::SharedMemCast));
        }
    }
}

// Generates code to assert that all fields implementing SharedMemCast
fn field_type_assertions(name: &Ident, data: &Data) -> TokenStream {
    match data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) if !fields.named.is_empty() => {
                type_assertions_from_types(fields.named.iter().map(|f| &f.ty))
            }
            Fields::Unnamed(fields) if !fields.unnamed.is_empty() => {
                type_assertions_from_types(fields.unnamed.iter().map(|f| &f.ty))
            }
            _ => {
                // Unit structs have no fields to assert anything for
                error(
                    name.span(),
                    "Zero-sized types cannot be casted from shared memory",
                )
            }
        },
        Data::Enum(data) => {
            if data.variants.is_empty() {
                return error(
                    name.span(),
                    "Empty enums types cannot be casted from shared memory",
                );
            }

            type_assertions_from_types(
                data.variants
                    .iter()
                    .flat_map(|var| var.fields.iter().map(|f| &f.ty)),
            )
        }
        //TODO: Maybe this could be added in the future if anyone needs it?
        Data::Union(data) => error(
            data.union_token.span(),
            "Untagged unions are not supported by SharedMemCast",
        ),
    }
}

/// For each type T, this expands to a line like this:
///
/// let _: shared_memory::AssertIsSharedMemCast<T>;
///
/// We take some care to use the span of each Type as the span of the corresponding line of
/// generated code. This way if one of the types does not implement SharedMemCast, the compiler's
/// error message underlines the location of that type in the user's code.
fn type_assertions_from_types<'a, I: Iterator<Item = &'a Type>>(types: I) -> TokenStream {
    let type_assertions = types.map(|ty| {
        quote_spanned! {ty.span() =>
            let _: shared_memory::AssertIsSharedMemCast<#ty>;
        }
    });

    quote! {
        #(#type_assertions)*
    }
}

/// Generates a compiler error message at the given span position
fn error<T: std::fmt::Display>(span: Span, message: T) -> TokenStream {
    syn::Error::new(span, message).to_compile_error()
}
