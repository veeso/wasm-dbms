use proc_macro2::TokenStream as TokenStream2;

use crate::table::metadata::TableMetadata;

/// Generate the foreign fetcher type, declared with `vis`, if the table has foreign keys.
pub fn generate_foreign_fetcher(vis: &syn::Visibility, metadata: &TableMetadata) -> TokenStream2 {
    let Some(foreign_fetcher) = metadata.foreign_fetcher.as_ref() else {
        return quote::quote! {};
    };

    let fetch_impl = impl_fetch(metadata);
    let fetch_batch_impl = impl_fetch_batch(metadata);

    quote::quote! {
        #[derive(Default)]
        #vis struct #foreign_fetcher;

        impl ::wasm_dbms_api::prelude::ForeignFetcher for #foreign_fetcher {
            #fetch_impl
            #fetch_batch_impl
        }
    }
}

/// Generates `ForeignFetcher::fetch`, which looks up one record of the referenced table by the
/// referenced column declared in `#[foreign_key(..., column = "...")]`.
fn impl_fetch(metadata: &TableMetadata) -> TokenStream2 {
    // match every (table, local column) relation we foreign fetch from
    let mut match_arms = vec![];
    for foreign in &metadata.foreign_keys {
        let table_name = &foreign.referenced_table.to_string();
        let local_column = &foreign.field.to_string();
        let referenced_column = &foreign.referenced_field.to_string();
        let entity_to_query = &foreign.entity;

        match_arms.push(quote::quote! {
            (#table_name, #local_column) => {
                let mut results = database.select::<#entity_to_query>(
                    ::wasm_dbms_api::prelude::Query::builder()
                        .all()
                        .limit(1)
                        .and_where(::wasm_dbms_api::prelude::Filter::Eq(#referenced_column.to_string(), value.clone()))
                        .build(),
                )?;
                let record = match results.pop() {
                    Some(record) => record,
                    None => {
                        return Err(::wasm_dbms_api::prelude::DbmsError::Query(::wasm_dbms_api::prelude::QueryError::BrokenForeignKeyReference {
                            table: #table_name.to_string(),
                            key: value,
                        }));
                    }
                };
                let values = record.to_values();
                Ok(vec![(
                    ::wasm_dbms_api::prelude::ValuesSource::Foreign {
                        table: #table_name.to_string(),
                        column: local_column.to_string(),
                    },
                    values,
                )])
            }
        });
    }

    let table_name = &metadata.name.to_string();

    quote::quote! {
        fn fetch(
            &self,
            database: &impl ::wasm_dbms_api::prelude::Database,
            table: &str,
            local_column: &'static str,
            value: ::wasm_dbms_api::prelude::Value,
        ) -> ::wasm_dbms_api::prelude::DbmsResult<::wasm_dbms_api::prelude::TableColumns> {
            use ::wasm_dbms_api::prelude::TableSchema as _;
            use ::wasm_dbms_api::prelude::TableRecord as _;

            match (table, local_column) {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(::wasm_dbms_api::prelude::QueryError::InvalidQuery(format!(
                    "ForeignFetcher: unknown relation '{table}.{local_column}' for {table_name} foreign fetcher",
                    table_name = #table_name
                )))),
            }
        }
    }
}

/// Generates `ForeignFetcher::fetch_batch`, which loads the records of the referenced table
/// whose referenced column holds one of the given values, keyed by that column.
fn impl_fetch_batch(metadata: &TableMetadata) -> TokenStream2 {
    let mut match_arms = vec![];
    for foreign in &metadata.foreign_keys {
        let table_name = &foreign.referenced_table.to_string();
        let local_column = &foreign.field.to_string();
        let referenced_column = &foreign.referenced_field.to_string();
        let entity_to_query = &foreign.entity;

        match_arms.push(quote::quote! {
            (#table_name, #local_column) => {
                let results = database.select::<#entity_to_query>(
                    ::wasm_dbms_api::prelude::Query::builder()
                        .all()
                        .and_where(::wasm_dbms_api::prelude::Filter::In(
                            #referenced_column.to_string(),
                            values.to_vec(),
                        ))
                        .build(),
                )?;
                results
                    .into_iter()
                    .map(|record| {
                        let record_values = record.to_values();
                        let key = record_values
                            .iter()
                            .find(|(col, _)| col.name == #referenced_column)
                            .map(|(_, value)| value.clone())
                            .ok_or_else(|| {
                                ::wasm_dbms_api::prelude::DbmsError::Query(
                                    ::wasm_dbms_api::prelude::QueryError::UnknownColumn(
                                        #referenced_column.to_string(),
                                    ),
                                )
                            })?;
                        Ok((key, record_values))
                    })
                    .collect()
            }
        });
    }

    let table_name = &metadata.name.to_string();

    quote::quote! {
        fn fetch_batch(
            &self,
            database: &impl ::wasm_dbms_api::prelude::Database,
            table: &str,
            local_column: &'static str,
            values: &[::wasm_dbms_api::prelude::Value],
        ) -> ::wasm_dbms_api::prelude::DbmsResult<
            ::std::collections::HashMap<::wasm_dbms_api::prelude::Value, Vec<(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)>>
        > {
            use ::wasm_dbms_api::prelude::TableSchema as _;
            use ::wasm_dbms_api::prelude::TableRecord as _;

            match (table, local_column) {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(::wasm_dbms_api::prelude::QueryError::InvalidQuery(format!(
                    "ForeignFetcher: unknown relation '{table}.{local_column}' for {table_name} foreign fetcher",
                    table_name = #table_name
                )))),
            }
        }
    }
}
