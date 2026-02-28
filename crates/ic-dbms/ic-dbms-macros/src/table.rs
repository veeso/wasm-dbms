mod foreign_fetcher;
mod insert;
mod metadata;
mod record;
mod table_schema;
mod update;

use proc_macro2::TokenStream as TokenStream2;
use syn::DeriveInput;

/// Generate implementation of the `TableSchema` trait for the given struct and all the types necessary for working with the ic-dbms-canister.
pub fn table(input: DeriveInput) -> syn::Result<TokenStream2> {
    let syn::Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            input.ident,
            "`Table` can only be derived for structs",
        ));
    };
    let metadata = self::metadata::collect_table_metadata(&input.ident, data, &input.attrs)?;
    let table_schema_tokens = self::table_schema::generate_table_schema(&input.ident, &metadata)?;
    let record_impl = self::record::generate_record(&input.ident, &metadata);
    let insert_impl = self::insert::generate_insert_request(&input.ident, &metadata);
    let update_impl = self::update::generate_update_request(&input.ident, &metadata);
    let foreign_fetcher_impl = self::foreign_fetcher::generate_foreign_fetcher(&metadata);
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
