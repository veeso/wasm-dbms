// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, X-NO-MOD-RS, M-CANONICAL-DOCS

mod metadata;

use proc_macro2::TokenStream as TokenStream2;
use syn::DeriveInput;

use self::metadata::TableEntry;

/// Entry point for the `#[derive(DatabaseSchema)]` macro.
///
/// Generates `impl<M, A> DatabaseSchema<M, A> for #struct` with match-arm
/// dispatch for all seven required trait methods, plus an inherent
/// `register_tables` helper.
pub fn database_schema(input: DeriveInput) -> syn::Result<TokenStream2> {
    let metadata = self::metadata::collect_schema_metadata(&input.attrs)?;
    let struct_ident = &input.ident;

    let database_schema_impl = impl_database_schema(struct_ident, &metadata.tables);
    let register_tables_impl = impl_register_tables(struct_ident, &metadata.tables);

    Ok(quote::quote! {
        #database_schema_impl
        #register_tables_impl
    })
}

/// Generates `impl<M, A> DatabaseSchema<M, A> for #struct_ident` with all
/// seven required trait methods.
fn impl_database_schema(struct_ident: &syn::Ident, tables: &[TableEntry]) -> TokenStream2 {
    let select_fn = impl_select(tables);
    let aggregate_fn = impl_aggregate(tables);
    let referenced_tables_fn = impl_referenced_tables(tables);
    let insert_fn = impl_insert(tables);
    let delete_fn = impl_delete(tables);
    let update_fn = impl_update(tables);
    let validate_insert_fn = impl_validate_insert(tables);
    let validate_update_fn = impl_validate_update(tables);
    let migrate_default_fn = impl_migrate_default(tables);
    let migrate_default_dyn_fn = impl_migrate_default_dyn();
    let migrate_transform_fn = impl_migrate_transform(tables);
    let migrate_transform_dyn_fn = impl_migrate_transform_dyn();
    let compiled_snapshots_fn = impl_compiled_snapshots(tables);
    let compiled_snapshots_dyn_fn = impl_compiled_snapshots_dyn();
    let renamed_from_dyn_fn = impl_renamed_from_dyn(tables);

    quote::quote! {
        impl<M, A> ::wasm_dbms::prelude::DatabaseSchema<M, A> for #struct_ident
        where
            M: ::wasm_dbms_memory::prelude::MemoryProvider,
            A: ::wasm_dbms_memory::prelude::AccessControl,
        {
            #select_fn
            #aggregate_fn
            #referenced_tables_fn
            #insert_fn
            #delete_fn
            #update_fn
            #validate_insert_fn
            #validate_update_fn
            #migrate_default_fn
            #migrate_default_dyn_fn
            #migrate_transform_fn
            #migrate_transform_dyn_fn
            #compiled_snapshots_fn
            #compiled_snapshots_dyn_fn
            #renamed_from_dyn_fn
        }
    }
}

/// Generates `impl #struct_ident { pub fn register_tables(...) }`.
fn impl_register_tables(struct_ident: &syn::Ident, tables: &[TableEntry]) -> TokenStream2 {
    let table_idents: Vec<_> = tables.iter().map(|t| &t.table).collect();

    quote::quote! {
        impl #struct_ident {
            /// Registers all tables managed by this schema in the given
            /// DBMS context.
            pub fn register_tables<M, A>(
                ctx: &::wasm_dbms::prelude::DbmsContext<M, A>,
            ) -> ::wasm_dbms_api::prelude::DbmsResult<()>
            where
                M: ::wasm_dbms_memory::prelude::MemoryProvider,
                A: ::wasm_dbms_memory::prelude::AccessControl,
            {
                #( ctx.register_table::<#table_idents>()?; )*
                Ok(())
            }
        }
    }
}

// -- Trait method generators ------------------------------------------------

fn impl_select(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                name if name == #entity::table_name() => {
                    let results = dbms.select_columns::<#entity>(query)?;
                    Ok(::wasm_dbms_api::prelude::flatten_table_columns(results))
                }
            }
        })
        .collect();

    quote::quote! {
        fn select(
            &self,
            dbms: &::wasm_dbms::prelude::WasmDbmsDatabase<'_, M, A>,
            table_name: &str,
            query: ::wasm_dbms_api::prelude::Query,
        ) -> ::wasm_dbms_api::prelude::DbmsResult<Vec<Vec<(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)>>> {
            use ::wasm_dbms_api::prelude::TableSchema as _;

            match table_name {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(
                    ::wasm_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    }
}

fn impl_aggregate(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                name if name == #entity::table_name() => {
                    dbms.aggregate::<#entity>(query, aggregates)
                }
            }
        })
        .collect();

    quote::quote! {
        fn aggregate(
            &self,
            dbms: &::wasm_dbms::prelude::WasmDbmsDatabase<'_, M, A>,
            table_name: &str,
            query: ::wasm_dbms_api::prelude::Query,
            aggregates: &[::wasm_dbms_api::prelude::AggregateFunction],
        ) -> ::wasm_dbms_api::prelude::DbmsResult<Vec<::wasm_dbms_api::prelude::AggregatedRow>> {
            use ::wasm_dbms_api::prelude::TableSchema as _;
            use ::wasm_dbms_api::prelude::Database as _;

            match table_name {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(
                    ::wasm_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    }
}

fn impl_referenced_tables(tables: &[TableEntry]) -> TokenStream2 {
    let table_tuples: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                (#entity::table_name(), #entity::columns())
            }
        })
        .collect();

    quote::quote! {
        fn referenced_tables(
            &self,
            table: &'static str,
        ) -> Vec<(&'static str, Vec<&'static str>)> {
            use ::wasm_dbms_api::prelude::TableSchema as _;
            let tables = &[
                #(#table_tuples),*
            ];
            ::wasm_dbms::prelude::get_referenced_tables(table, tables)
        }
    }
}

fn impl_insert(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            let insert = &t.insert;
            quote::quote! {
                name if name == #entity::table_name() => {
                    let insert_request = #insert::from_values(record_values)?;
                    dbms.insert::<#entity>(insert_request)
                }
            }
        })
        .collect();

    quote::quote! {
        fn insert(
            &self,
            dbms: &::wasm_dbms::prelude::WasmDbmsDatabase<'_, M, A>,
            table_name: &'static str,
            record_values: &[(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)],
        ) -> ::wasm_dbms_api::prelude::DbmsResult<()> {
            use ::wasm_dbms_api::prelude::TableSchema as _;
            use ::wasm_dbms_api::prelude::InsertRecord as _;
            use ::wasm_dbms_api::prelude::Database as _;

            match table_name {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(
                    ::wasm_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    }
}

fn impl_delete(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                name if name == #entity::table_name() => {
                    dbms.delete::<#entity>(delete_behavior, filter)
                }
            }
        })
        .collect();

    quote::quote! {
        fn delete(
            &self,
            dbms: &::wasm_dbms::prelude::WasmDbmsDatabase<'_, M, A>,
            table_name: &'static str,
            delete_behavior: ::wasm_dbms_api::prelude::DeleteBehavior,
            filter: Option<::wasm_dbms_api::prelude::Filter>,
        ) -> ::wasm_dbms_api::prelude::DbmsResult<u64> {
            use ::wasm_dbms_api::prelude::TableSchema as _;
            use ::wasm_dbms_api::prelude::Database as _;

            match table_name {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(
                    ::wasm_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    }
}

fn impl_update(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            let update = &t.update;
            quote::quote! {
                name if name == #entity::table_name() => {
                    let update_request = #update::from_values(patch_values, filter);
                    dbms.update::<#entity>(update_request)
                }
            }
        })
        .collect();

    quote::quote! {
        fn update(
            &self,
            dbms: &::wasm_dbms::prelude::WasmDbmsDatabase<'_, M, A>,
            table_name: &'static str,
            patch_values: &[(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)],
            filter: Option<::wasm_dbms_api::prelude::Filter>,
        ) -> ::wasm_dbms_api::prelude::DbmsResult<u64> {
            use ::wasm_dbms_api::prelude::TableSchema as _;
            use ::wasm_dbms_api::prelude::UpdateRecord as _;
            use ::wasm_dbms_api::prelude::Database as _;

            match table_name {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(
                    ::wasm_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    }
}

fn impl_validate_insert(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                name if name == #entity::table_name() => {
                    ::wasm_dbms::prelude::InsertIntegrityValidator::<#entity, M, A>::new(dbms).validate(record_values)
                }
            }
        })
        .collect();

    quote::quote! {
        fn validate_insert(
            &self,
            dbms: &::wasm_dbms::prelude::WasmDbmsDatabase<'_, M, A>,
            table_name: &'static str,
            record_values: &[(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)],
        ) -> ::wasm_dbms_api::prelude::DbmsResult<()> {
            use ::wasm_dbms_api::prelude::TableSchema as _;

            match table_name {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(
                    ::wasm_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    }
}

fn impl_migrate_default(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                name if name == <#entity as ::wasm_dbms_api::prelude::TableSchema>::table_name() => {
                    if let Some(value) =
                        <#entity as ::wasm_dbms_api::prelude::Migrate>::default_value(column)
                    {
                        return Some(value);
                    }
                    <#entity as ::wasm_dbms_api::prelude::TableSchema>::columns()
                        .iter()
                        .find(|c| c.name == column)
                        .and_then(|c| c.default.map(|f| f()))
                }
            }
        })
        .collect();

    quote::quote! {
        fn migrate_default(table: &str, column: &str) -> Option<::wasm_dbms_api::prelude::Value> {
            match table {
                #(#match_arms)*
                _ => None,
            }
        }
    }
}

fn impl_migrate_transform(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                name if name == <#entity as ::wasm_dbms_api::prelude::TableSchema>::table_name() => {
                    <#entity as ::wasm_dbms_api::prelude::Migrate>::transform_column(column, old)
                }
            }
        })
        .collect();

    quote::quote! {
        fn migrate_transform(
            table: &str,
            column: &str,
            old: ::wasm_dbms_api::prelude::Value,
        ) -> ::wasm_dbms_api::prelude::DbmsResult<Option<::wasm_dbms_api::prelude::Value>> {
            match table {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(
                    ::wasm_dbms_api::prelude::QueryError::TableNotFound(table.to_string()),
                )),
            }
        }
    }
}

fn impl_compiled_snapshots(tables: &[TableEntry]) -> TokenStream2 {
    let entries: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                <#entity as ::wasm_dbms_api::prelude::TableSchema>::schema_snapshot()
            }
        })
        .collect();

    quote::quote! {
        fn compiled_snapshots() -> Vec<::wasm_dbms_api::prelude::TableSchemaSnapshot> {
            vec![#(#entries),*]
        }
    }
}

fn impl_compiled_snapshots_dyn() -> TokenStream2 {
    quote::quote! {
        fn compiled_snapshots_dyn(&self) -> Vec<::wasm_dbms_api::prelude::TableSchemaSnapshot> {
            <Self as ::wasm_dbms::prelude::DatabaseSchema<M, A>>::compiled_snapshots()
        }
    }
}

fn impl_migrate_default_dyn() -> TokenStream2 {
    quote::quote! {
        fn migrate_default_dyn(
            &self,
            table: &str,
            column: &str,
        ) -> Option<::wasm_dbms_api::prelude::Value> {
            <Self as ::wasm_dbms::prelude::DatabaseSchema<M, A>>::migrate_default(table, column)
        }
    }
}

fn impl_migrate_transform_dyn() -> TokenStream2 {
    quote::quote! {
        fn migrate_transform_dyn(
            &self,
            table: &str,
            column: &str,
            old: ::wasm_dbms_api::prelude::Value,
        ) -> ::wasm_dbms_api::prelude::DbmsResult<Option<::wasm_dbms_api::prelude::Value>> {
            <Self as ::wasm_dbms::prelude::DatabaseSchema<M, A>>::migrate_transform(table, column, old)
        }
    }
}

fn impl_renamed_from_dyn(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                name if name == <#entity as ::wasm_dbms_api::prelude::TableSchema>::table_name() => {
                    <#entity as ::wasm_dbms_api::prelude::TableSchema>::columns()
                        .iter()
                        .find(|c| c.name == column)
                        .map(|c| c.renamed_from.to_vec())
                        .unwrap_or_default()
                }
            }
        })
        .collect();

    quote::quote! {
        fn renamed_from_dyn(&self, table: &str, column: &str) -> Vec<&'static str> {
            match table {
                #(#match_arms)*
                _ => Vec::new(),
            }
        }
    }
}

fn impl_validate_update(tables: &[TableEntry]) -> TokenStream2 {
    let match_arms: Vec<_> = tables
        .iter()
        .map(|t| {
            let entity = &t.table;
            quote::quote! {
                name if name == #entity::table_name() => {
                    ::wasm_dbms::prelude::UpdateIntegrityValidator::<#entity, M, A>::new(dbms, old_pk).validate(record_values)
                }
            }
        })
        .collect();

    quote::quote! {
        fn validate_update(
            &self,
            dbms: &::wasm_dbms::prelude::WasmDbmsDatabase<'_, M, A>,
            table_name: &'static str,
            record_values: &[(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)],
            old_pk: ::wasm_dbms_api::prelude::Value,
        ) -> ::wasm_dbms_api::prelude::DbmsResult<()> {
            use ::wasm_dbms_api::prelude::TableSchema as _;

            match table_name {
                #(#match_arms)*
                _ => Err(::wasm_dbms_api::prelude::DbmsError::Query(
                    ::wasm_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    }
}
