use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Result};

pub fn custom_data_type(input: &DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Extract #[type_tag = "..."] attribute (NameValue syntax, matching how #[table = "..."] is parsed)
    let type_tag_str = extract_type_tag(input)?;

    Ok(quote! {
        impl #impl_generics ::ic_dbms_api::prelude::CustomDataType for #name #ty_generics #where_clause {
            const TYPE_TAG: &'static str = #type_tag_str;
        }

        impl #impl_generics ::std::convert::From<#name #ty_generics> for ::ic_dbms_api::prelude::Value #where_clause {
            fn from(val: #name #ty_generics) -> ::ic_dbms_api::prelude::Value {
                ::ic_dbms_api::prelude::Value::Custom(::ic_dbms_api::prelude::CustomValue::new(&val))
            }
        }
    })
}

/// Extract the type tag string from `#[type_tag = "..."]` attribute.
///
/// Uses the same NameValue parsing pattern as `#[table = "..."]` in `metadata.rs`.
fn extract_type_tag(input: &DeriveInput) -> Result<String> {
    for attr in &input.attrs {
        if attr.path().is_ident("type_tag") {
            let name_value = attr.meta.require_name_value().map_err(|_| {
                syn::Error::new_spanned(attr, "expected `#[type_tag = \"...\"]` syntax")
            })?;

            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit),
                ..
            }) = &name_value.value
            {
                return Ok(lit.value());
            } else {
                return Err(syn::Error::new_spanned(
                    &name_value.value,
                    "expected string literal for type_tag",
                ));
            }
        }
    }

    Err(syn::Error::new_spanned(
        input,
        "CustomDataType requires a `#[type_tag = \"...\"]` attribute",
    ))
}
