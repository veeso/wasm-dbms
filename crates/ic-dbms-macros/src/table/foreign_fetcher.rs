use proc_macro2::TokenStream as TokenStream2;

use crate::table::metadata::TableMetadata;

pub fn generate_foreign_fetcher(metadata: &TableMetadata) -> TokenStream2 {
    let Some(foreign_fetcher) = metadata.foreign_fetcher.as_ref() else {
        return quote::quote! {};
    };

    let fetch_impl = impl_fetch(metadata);

    quote::quote! {
        #[derive(Default)]
        pub struct #foreign_fetcher;

        impl ::ic_dbms_api::prelude::ForeignFetcher for #foreign_fetcher {
            #fetch_impl
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
                    ::ic_dbms_api::prelude::Query::builder()
                        .all()
                        .limit(1)
                        .and_where(::ic_dbms_api::prelude::Filter::Eq(#pk_call.to_string(), pk_value.clone()))
                        .build(),
                )?;
                let record = match results.pop() {
                    Some(record) => record,
                    None => {
                        return Err(::ic_dbms_api::prelude::IcDbmsError::Query(::ic_dbms_api::prelude::QueryError::BrokenForeignKeyReference {
                            table: #table_name.to_string(),
                            key: pk_value,
                        }));
                    }
                };
                let values = record.to_values();
                Ok(vec![(
                    ::ic_dbms_api::prelude::ValuesSource::Foreign {
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
            database: &impl ::ic_dbms_api::prelude::Database,
            table: &str,
            local_column: &'static str,
            pk_value: ::ic_dbms_api::prelude::Value,
        ) -> ic_dbms_api::prelude::IcDbmsResult<::ic_dbms_api::prelude::TableColumns> {
            use ::ic_dbms_api::prelude::TableSchema as _;
            use ::ic_dbms_api::prelude::TableRecord as _;

            match table {
                #(#match_arms)*
                _ => Err(ic_dbms_api::prelude::IcDbmsError::Query(ic_dbms_api::prelude::QueryError::InvalidQuery(format!(
                    "ForeignFetcher: unknown table '{table}' for {table_name} foreign fetcher",
                    table_name = #table_name
                )))),
            }
        }
    }
}
