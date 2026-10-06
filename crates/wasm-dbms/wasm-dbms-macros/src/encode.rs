use proc_macro2::TokenStream as TokenStream2;
use syn::{DataStruct, DeriveInput};

use crate::utils;

/// Generate implementation of `Encode` trait.
///
/// If `alignment` is `Some` and the data size is not `FIXED`, the alignment will be set to the provided value.
pub fn encode(
    DeriveInput {
        ident,
        data,
        generics,
        ..
    }: DeriveInput,
    alignment: Option<u16>,
) -> syn::Result<TokenStream2> {
    let syn::Data::Struct(struct_data) = data else {
        return Err(syn::Error::new_spanned(
            ident,
            "`Encode` can only be derived for structs",
        ));
    };

    let data_size = impl_size_const(&struct_data);
    let alignment = impl_alignment_const(&struct_data, alignment);
    let size = impl_size(&struct_data);
    let encode = impl_encode(&struct_data);
    let decode = impl_decode(&struct_data);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    Ok(quote::quote! {
        impl #impl_generics ::wasm_dbms_api::prelude::Encode for #ident #ty_generics #where_clause {
            const SIZE: ::wasm_dbms_api::prelude::DataSize = #data_size;
            const ALIGNMENT: ::wasm_dbms_api::prelude::PageOffset = #alignment;

            #size

            #encode

            #decode
        }
    })
}

/// Generate implementation of `SIZE` const value.
///
/// A struct without fields has the fixed size `0`.
fn impl_size_const(struct_data: &DataStruct) -> TokenStream2 {
    if struct_data.fields.is_empty() {
        return quote::quote! { ::wasm_dbms_api::prelude::DataSize::Fixed(0) };
    }

    let tuple_expansion = size_tuple_expansion(struct_data);

    let anon_idents = utils::anon_ident_iter(None)
        .take(struct_data.fields.len())
        .collect::<Vec<_>>();

    // extract sizes from fields
    quote::quote! {
        if let (#(::wasm_dbms_api::prelude::DataSize::Fixed(#anon_idents)),*) = (#tuple_expansion) {
            let total_size = #(#anon_idents)+*;
            ::wasm_dbms_api::prelude::DataSize::Fixed(total_size)
        } else {
            ::wasm_dbms_api::prelude::DataSize::Dynamic
        }
    }
}

/// Generate implementation of `ALIGNMENT` const value.
///
/// If `alignment` is `Some` and the data size is not `FIXED`, the alignment will be set to the provided value.
///
/// A fixed-size struct is aligned to its size; a zero-size struct (no fields, or only zero-size
/// fields) is aligned to `1`, because storage divides offsets by the alignment.
fn impl_alignment_const(struct_data: &DataStruct, alignment: Option<u16>) -> TokenStream2 {
    if struct_data.fields.is_empty() {
        return quote::quote! { 1 };
    }

    let tuple_expansion = size_tuple_expansion(struct_data);

    let anon_idents = utils::anon_ident_iter(None)
        .take(struct_data.fields.len())
        .collect::<Vec<_>>();

    let quoted_alignment_value = match alignment {
        Some(alignment) => quote::quote! { #alignment },
        None => quote::quote! { ::wasm_dbms_api::prelude::DEFAULT_ALIGNMENT },
    };

    // extract sizes from fields
    quote::quote! {
        if let (#(::wasm_dbms_api::prelude::DataSize::Fixed(#anon_idents)),*) = (#tuple_expansion) {
            let total_size = #(#anon_idents)+*;
            if total_size == 0 { 1 } else { total_size }
        }
        else {
            #quoted_alignment_value
        }
    }
}

/// Generate tuple expansion of field sizes.
fn size_tuple_expansion(struct_data: &DataStruct) -> TokenStream2 {
    let items = struct_data.fields.iter().map(|field| {
        let field_ty = &field.ty;
        quote::quote! {
            <#field_ty as ::wasm_dbms_api::prelude::Encode>::SIZE
        }
    });
    quote::quote! { #(#items),* }
}

/// Generate implementation of `size` method.
fn impl_size(struct_data: &DataStruct) -> TokenStream2 {
    let items = struct_data.fields.iter().map(|field| {
        let field_name = &field.ident;
        let field_ty = &field.ty;

        quote::quote! {
            <#field_ty as ::wasm_dbms_api::prelude::Encode>::size(&self.#field_name)
        }
    });

    // Saturate instead of overflowing: a record whose size does not fit an
    // `MSize` reports `MSize::MAX`, which storage rejects before encoding.
    quote::quote! {
        fn size(&self) -> ::wasm_dbms_api::prelude::MSize {
            ::wasm_dbms_api::prelude::MSize::MIN #( .saturating_add(#items) )*
        }
    }
}

/// Generate implementation of `encode` method.
fn impl_encode(struct_data: &DataStruct) -> TokenStream2 {
    if struct_data.fields.is_empty() {
        return quote::quote! {
            fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
                std::borrow::Cow::Borrowed(&[])
            }
        };
    }

    // make token for each field for encoding
    let encodings = struct_data.fields.iter().map(|field| {
        let field_ty = &field.ty;
        let field_name = &field.ident;

        quote::quote! {
            __wasm_dbms_encoded.extend_from_slice(&<#field_ty as ::wasm_dbms_api::prelude::Encode>::encode(&self.#field_name));
        }
    });

    quote::quote! {
        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            let mut __wasm_dbms_encoded = Vec::with_capacity(self.size() as usize);
            #(#encodings)*
            std::borrow::Cow::Owned(__wasm_dbms_encoded)
        }
    }
}

/// Generate implementation of `decode` method.
fn impl_decode(struct_data: &DataStruct) -> TokenStream2 {
    if struct_data.fields.is_empty() {
        // `Self {}` is valid for both empty named structs and unit structs
        return quote::quote! {
            fn decode(_: std::borrow::Cow<[u8]>) -> ::wasm_dbms_api::prelude::MemoryResult<Self> {
                Ok(Self {})
            }
        };
    }

    let decodings = struct_data.fields.iter().map(|field| {
        let field_name = &field.ident;
        let field_ty = &field.ty;

        quote::quote! {
            let #field_name = <#field_ty as ::wasm_dbms_api::prelude::Encode>::decode(std::borrow::Cow::Borrowed(&__wasm_dbms_data[__wasm_dbms_offset..]))?;
            __wasm_dbms_offset += <#field_ty as ::wasm_dbms_api::prelude::Encode>::size(&#field_name) as usize;
        }
    });

    let field_names = struct_data
        .fields
        .iter()
        .map(|field| &field.ident)
        .collect::<Vec<_>>();

    // locals are prefixed so that they cannot be shadowed by the field bindings
    quote::quote! {
        fn decode(__wasm_dbms_data: std::borrow::Cow<[u8]>) -> ::wasm_dbms_api::prelude::MemoryResult<Self> {
            let mut __wasm_dbms_offset = 0;
            #(#decodings)*

            Ok(Self {
                #(#field_names),*
            })
        }
    }
}
