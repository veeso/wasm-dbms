mod foreign_fetcher;
mod insert;
mod metadata;
mod record;
mod table_schema;
mod update;

use proc_macro2::TokenStream as TokenStream2;
use syn::DeriveInput;

/// Generate implementation of the `TableSchema` trait for the given struct and all the types necessary for working with the wasm-dbms engine.
///
/// Type parameters and `where` clauses of the struct are carried over to every generated type
/// and impl; lifetime and const generic parameters are rejected.
pub fn table(input: DeriveInput) -> syn::Result<TokenStream2> {
    let syn::Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            input.ident,
            "`Table` can only be derived for structs",
        ));
    };
    crate::utils::reject_unsupported_generics(&input.generics, "Table")?;
    let metadata = self::metadata::collect_table_metadata(&input.ident, data, &input.attrs)?;
    // every generated item carries the struct generics; `TableSchema` requires `'static`
    let generics = crate::utils::with_static_bounds(&input.generics);
    let table_schema_tokens =
        self::table_schema::generate_table_schema(&input.ident, &generics, &metadata)?;
    let record_impl = self::record::generate_record(&input.ident, &input.vis, &generics, &metadata);
    let insert_impl =
        self::insert::generate_insert_request(&input.ident, &input.vis, &generics, &metadata);
    let update_impl =
        self::update::generate_update_request(&input.ident, &input.vis, &generics, &metadata);
    let foreign_fetcher_impl =
        self::foreign_fetcher::generate_foreign_fetcher(&input.vis, &metadata);
    let encode_impl = crate::encode::encode(input, metadata.alignment)?;

    Ok(quote::quote! {
        #table_schema_tokens
        #encode_impl
        #record_impl
        #insert_impl
        #update_impl
        #foreign_fetcher_impl
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_error(input: DeriveInput) -> String {
        match table(input) {
            Ok(_) => panic!("`Table` derive should reject the input"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn test_should_reject_lifetime_parameters() {
        let message = table_error(syn::parse_quote! {
            #[table = "borrowed"]
            struct Borrowed<'a> {
                #[primary_key]
                id: Uint32,
                marker: std::marker::PhantomData<&'a ()>,
            }
        });
        assert_eq!(message, "`Table` does not support lifetime parameters");
    }

    #[test]
    fn test_should_reject_const_generic_parameters() {
        let message = table_error(syn::parse_quote! {
            #[table = "sized"]
            struct Sized<const N: usize> {
                #[primary_key]
                id: Uint32,
            }
        });
        assert_eq!(message, "`Table` does not support const generic parameters");
    }
}
