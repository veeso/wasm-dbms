use proc_macro2::TokenStream as TokenStream2;

use crate::table::metadata::TableMetadata;

pub fn generate_foreign_fetcher(metadata: &TableMetadata) -> TokenStream2 {
    let Some(foreign_fetcher) = metadata.foreign_fetcher.as_ref() else {
        return quote::quote! {};
    };

    let fetch_impl = impl_fetch(metadata);
    let fetch_batch_impl = impl_fetch_batch(metadata);

    quote::quote! {
        #[derive(Default)]
        pub struct #foreign_fetcher;

        impl ::wasm_dbms_api::prelude::ForeignFetcher for #foreign_fetcher {
            #fetch_impl
            #fetch_batch_impl
        }
    }
}

fn impl_fetch(metadata: &TableMetadata) -> TokenStream2 {
    // match every table we foreign fetch from
    let mut match_arms = vec![];
    for foreign in &metadata.foreign_keys {
        let table_name = &foreign.referenced_table.to_string();
        let entity_to_query = &foreign.entity;
        let pk_call = quote::quote! { #entity_to_query::primary_key() };

        match_arms.push(quote::quote! {
            #table_name => {
                let mut results = database.select::<#entity_to_query>(
                    ::wasm_dbms_api::prelude::Query::builder()
                        .all()
                        .limit(1)
                        .and_where(::wasm_dbms_api::prelude::Filter::Eq(#pk_call.to_string(), pk_value.clone()))
                        .build(),
                )?;
                let record = match results.pop() {
                    Some(record) => record,
                    None => {
                        return Err(::wasm_dbms_api::prelude::DbmsError::Query(::wasm_dbms_api::prelude::QueryError::BrokenForeignKeyReference {
                            table: #table_name.to_string(),
                            key: pk_value,
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
            pk_value: ::wasm_dbms_api::prelude::Value,
        ) -> wasm_dbms_api::prelude::DbmsResult<::wasm_dbms_api::prelude::TableColumns> {
            use ::wasm_dbms_api::prelude::TableSchema as _;
            use ::wasm_dbms_api::prelude::TableRecord as _;

            match table {
                #(#match_arms)*
                _ => Err(wasm_dbms_api::prelude::DbmsError::Query(wasm_dbms_api::prelude::QueryError::InvalidQuery(format!(
                    "ForeignFetcher: unknown table '{table}' for {table_name} foreign fetcher",
                    table_name = #table_name
                )))),
            }
        }
    }
}

fn impl_fetch_batch(metadata: &TableMetadata) -> TokenStream2 {
    let mut match_arms = vec![];
    for foreign in &metadata.foreign_keys {
        let table_name = &foreign.referenced_table.to_string();
        let entity_to_query = &foreign.entity;
        let pk_call = quote::quote! { #entity_to_query::primary_key() };

        match_arms.push(quote::quote! {
            #table_name => {
                let pk_field = #pk_call.to_string();
                let results = database.select::<#entity_to_query>(
                    ::wasm_dbms_api::prelude::Query::builder()
                        .all()
                        .and_where(::wasm_dbms_api::prelude::Filter::In(
                            pk_field.clone(),
                            pk_values.to_vec(),
                        ))
                        .build(),
                )?;
                let map = results
                    .into_iter()
                    .map(|record| {
                        let values = record.to_values();
                        let pk = values
                            .iter()
                            .find(|(col, _)| col.name == pk_field)
                            .expect("primary key column not found in foreign record")
                            .1
                            .clone();
                        (pk, values)
                    })
                    .collect::<::std::collections::HashMap<_, _>>();
                Ok(map)
            }
        });
    }

    let table_name = &metadata.name.to_string();

    quote::quote! {
        fn fetch_batch(
            &self,
            database: &impl ::wasm_dbms_api::prelude::Database,
            table: &str,
            pk_values: &[::wasm_dbms_api::prelude::Value],
        ) -> wasm_dbms_api::prelude::DbmsResult<
            ::std::collections::HashMap<::wasm_dbms_api::prelude::Value, Vec<(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)>>
        > {
            use ::wasm_dbms_api::prelude::TableSchema as _;
            use ::wasm_dbms_api::prelude::TableRecord as _;

            match table {
                #(#match_arms)*
                _ => Err(wasm_dbms_api::prelude::DbmsError::Query(wasm_dbms_api::prelude::QueryError::InvalidQuery(format!(
                    "ForeignFetcher: unknown table '{table}' for {table_name} foreign fetcher",
                    table_name = #table_name
                )))),
            }
        }
    }
}
