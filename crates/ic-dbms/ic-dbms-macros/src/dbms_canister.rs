mod metadata;

use proc_macro2::TokenStream as TokenStream2;
use quote::format_ident;
use syn::DeriveInput;

use self::metadata::TableMetadata;

pub fn dbms_canister(input: DeriveInput) -> syn::Result<TokenStream2> {
    let metadata = self::metadata::collect_canister_metadata(&input.attrs)?;
    let struct_ident = &input.ident;

    let database_schema_impl = impl_database_schema(struct_ident, &metadata.tables);
    let init_fn = impl_init(&metadata.tables);
    let inspect_fn = impl_inspect();
    let acl_api = impl_acl_api();
    let transaction_api = impl_transaction_api(struct_ident);
    let tables_api = impl_tables_api(&metadata.tables, struct_ident);
    let select_raw_api = impl_select_raw_api(struct_ident);

    Ok(quote::quote! {
        #database_schema_impl
        #init_fn
        #inspect_fn
        #acl_api
        #transaction_api
        #tables_api
        #select_raw_api
    })
}

fn impl_init(tables: &[TableMetadata]) -> TokenStream2 {
    let mut init_tables = vec![];
    for table in tables {
        let table_name = &table.table;
        let table_str = table_name.to_string();
        init_tables.push(quote::quote! {
            ::ic_dbms_canister::prelude::DBMS_CONTEXT.with(|ctx| {
                if let Err(err) = ctx.register_table::<#table_name>() {
                    ::ic_cdk::trap(&format!(
                        "Failed to register table {} during init: {}",
                        #table_str, err
                    ));
                }
            });
        });
    }

    quote::quote! {
        #[::ic_cdk::init]
        fn init(args: ::ic_dbms_api::prelude::IcDbmsCanisterArgs) {
            let args = args.unwrap_init();
            ::ic_dbms_canister::prelude::DBMS_CONTEXT.with(|ctx| {
                for principal in args.allowed_principals {
                    let identity = principal.as_slice().to_vec();
                    if let Err(err) = ctx.acl_add(identity) {
                        ::ic_cdk::trap(&format!(
                            "Failed to add principal to ACL during init: {}",
                            err
                        ));
                    }
                }
            });
            #(#init_tables)*
        }
    }
}

fn impl_acl_api() -> TokenStream2 {
    quote::quote! {
        #[::ic_cdk::update]
        fn acl_add_principal(principal: ::candid::Principal) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            ::ic_dbms_canister::api::acl_add_principal(principal)
        }

        #[::ic_cdk::update]
        fn acl_remove_principal(principal: ::candid::Principal) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            ::ic_dbms_canister::api::acl_remove_principal(principal)
        }

        #[::ic_cdk::query]
        fn acl_allowed_principals() -> Vec<::candid::Principal> {
            ::ic_dbms_canister::api::acl_allowed_principals()
        }
    }
}

fn impl_inspect() -> TokenStream2 {
    quote::quote! {
        #[::ic_cdk::inspect_message]
        fn inspect() {
            ::ic_dbms_canister::api::inspect()
        }
    }
}

fn impl_tables_api(tables: &[TableMetadata], struct_ident: &syn::Ident) -> TokenStream2 {
    let table_apis: Vec<_> = tables
        .iter()
        .map(|table| impl_table_api(table, struct_ident))
        .collect();

    quote::quote! {
        #(#table_apis)*
    }
}

fn impl_transaction_api(struct_ident: &syn::Ident) -> TokenStream2 {
    quote::quote! {
        #[::ic_cdk::update]
        fn begin_transaction() -> ::ic_dbms_api::prelude::TransactionId {
            ::ic_dbms_canister::api::begin_transaction()
        }

        #[::ic_cdk::update]
        fn commit(transaction_id: ::ic_dbms_api::prelude::TransactionId) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            ::ic_dbms_canister::api::commit(transaction_id, #struct_ident)
        }

        #[::ic_cdk::update]
        fn rollback(transaction_id: ::ic_dbms_api::prelude::TransactionId) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            ::ic_dbms_canister::api::rollback(transaction_id, #struct_ident)
        }
    }
}

fn impl_select_raw_api(struct_ident: &syn::Ident) -> TokenStream2 {
    quote::quote! {
        #[::ic_cdk::query]
        fn select(
            table: String,
            query: ::ic_dbms_api::prelude::Query,
            transaction_id: Option<::ic_dbms_api::prelude::TransactionId>,
        ) -> ::ic_dbms_api::prelude::IcDbmsResult<Vec<Vec<(::ic_dbms_api::prelude::CandidColumnDef, ::ic_dbms_api::prelude::Value)>>> {
            if query.has_joins() {
                ::ic_dbms_canister::api::select_join(&table, query, transaction_id, #struct_ident)
            } else {
                ::ic_dbms_canister::api::select_raw(&table, query, transaction_id, #struct_ident)
                    .map(|rows| {
                        rows.into_iter()
                            .map(|row| {
                                row.into_iter()
                                    .map(|(col, val)| (::ic_dbms_api::prelude::CandidColumnDef::from(col), val))
                                    .collect()
                            })
                            .collect()
                    })
            }
        }
    }
}

fn impl_table_api(table: &TableMetadata, struct_ident: &syn::Ident) -> TokenStream2 {
    let table_name = &table.name;
    let entity = &table.table;
    let record = &table.record;
    let insert = &table.insert;
    let update = &table.update;
    let select_fn_name = format_ident!("select_{}", table_name);
    let insert_fn_name = format_ident!("insert_{}", table_name);
    let update_fn_name = format_ident!("update_{}", table_name);
    let delete_fn_name = format_ident!("delete_{}", table_name);

    quote::quote! {
        #[::ic_cdk::query]
        fn #select_fn_name(query: ::ic_dbms_api::prelude::Query, transaction_id: Option<::ic_dbms_api::prelude::TransactionId>) -> ::ic_dbms_api::prelude::IcDbmsResult<Vec<#record>> {
            ::ic_dbms_canister::api::select::<#entity>(query, transaction_id, #struct_ident)
        }

        #[::ic_cdk::update]
        fn #insert_fn_name(record: #insert, transaction_id: Option<::ic_dbms_api::prelude::TransactionId>) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            ::ic_dbms_canister::api::insert::<#entity>(record, transaction_id, #struct_ident)
        }

        #[::ic_cdk::update]
        fn #update_fn_name(patch: #update, transaction_id: Option<::ic_dbms_api::prelude::TransactionId>) -> ::ic_dbms_api::prelude::IcDbmsResult<u64> {
            ::ic_dbms_canister::api::update::<#entity>(patch, transaction_id, #struct_ident)
        }

        #[::ic_cdk::update]
        fn #delete_fn_name(delete_behavior: ::ic_dbms_api::prelude::DeleteBehavior, filter: Option<::ic_dbms_api::prelude::Filter>, transaction_id: Option<::ic_dbms_api::prelude::TransactionId>) -> ::ic_dbms_api::prelude::IcDbmsResult<u64> {
            ::ic_dbms_canister::api::delete::<#entity>(delete_behavior, filter, transaction_id, #struct_ident)
        }
    }
}

fn impl_database_schema(
    struct_ident: &syn::Ident,
    tables: &[TableMetadata],
) -> TokenStream2 {
    let mut tables_for_ref = vec![];
    for table in tables {
        let entity = &table.table;
        tables_for_ref.push(quote::quote! {
            (
                #entity::table_name(),
                #entity::columns(),
            )
        });
    }

    let mut select_match_arms = vec![];
    for table in tables {
        let table_name = &table.table;
        select_match_arms.push(quote::quote! {
            name if name == #table_name::table_name() => {
                let results = dbms.select_columns::<#table_name>(query)?;
                Ok(::ic_dbms_api::prelude::flatten_table_columns(results))
            }
        });
    }

    let select_fn = quote::quote! {
        fn select(
            &self,
            dbms: &::ic_dbms_canister::prelude::WasmDbmsDatabase<'_, M>,
            table_name: &str,
            query: ::ic_dbms_api::prelude::Query,
        ) -> ::ic_dbms_api::prelude::IcDbmsResult<Vec<Vec<(::ic_dbms_api::prelude::ColumnDef, ::ic_dbms_api::prelude::Value)>>> {
            use ::ic_dbms_api::prelude::TableSchema as _;

            match table_name {
                #(#select_match_arms)*
                _ => Err(::ic_dbms_api::prelude::IcDbmsError::Query(
                    ::ic_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    };

    let referenced_tables_fn = quote::quote! {
        fn referenced_tables(
            &self,
            table: &'static str,
        ) -> Vec<(&'static str, Vec<&'static str>)> {
            use ::ic_dbms_api::prelude::TableSchema as _;
            let tables = &[
                #(#tables_for_ref),*
            ];
            ::ic_dbms_canister::prelude::get_referenced_tables(table, tables)
        }
    };

    let mut insert_match_arms = vec![];
    for table in tables {
        let insert_name = &table.insert;
        let table_name = &table.table;
        insert_match_arms.push(quote::quote! {
            name if name == #table_name::table_name() => {
                let insert_request = #insert_name::from_values(record_values)?;
                dbms.insert::<#table_name>(insert_request)
            }
        });
    }

    let insert_tables_fn = quote::quote! {
        fn insert(
            &self,
            dbms: &::ic_dbms_canister::prelude::WasmDbmsDatabase<'_, M>,
            table_name: &'static str,
            record_values: &[(::ic_dbms_api::prelude::ColumnDef, ::ic_dbms_api::prelude::Value)],
        ) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            use ::ic_dbms_api::prelude::TableSchema as _;
            use ::ic_dbms_api::prelude::InsertRecord as _;
            use ::ic_dbms_api::prelude::Database as _;

            match table_name {
                #(#insert_match_arms)*
                _ => Err(::ic_dbms_api::prelude::IcDbmsError::Query(
                    ::ic_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    };

    let mut delete_tables_arms = vec![];
    for table in tables {
        let table_name = &table.table;
        delete_tables_arms.push(quote::quote! {
            name if name == #table_name::table_name() => {
                dbms.delete::<#table_name>(delete_behavior, filter)
            }
        });
    }

    let delete_tables_fn = quote::quote! {
        fn delete(
            &self,
            dbms: &::ic_dbms_canister::prelude::WasmDbmsDatabase<'_, M>,
            table_name: &'static str,
            delete_behavior: ::ic_dbms_api::prelude::DeleteBehavior,
            filter: Option<::ic_dbms_api::prelude::Filter>,
        ) -> ::ic_dbms_api::prelude::IcDbmsResult<u64> {
            use ::ic_dbms_api::prelude::TableSchema as _;
            use ::ic_dbms_api::prelude::Database as _;
            match table_name {
                #(#delete_tables_arms)*
                _ => Err(::ic_dbms_api::prelude::IcDbmsError::Query(
                    ::ic_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    };

    let mut update_tables_arms = vec![];
    for table in tables {
        let table_name = &table.table;
        let update_name = &table.update;
        update_tables_arms.push(quote::quote! {
            name if name == #table_name::table_name() => {
                let update_request = #update_name::from_values(patch_values, filter);
                dbms.update::<#table_name>(update_request)
            }
        });
    }

    let update_tables_fn = quote::quote! {
        fn update(
            &self,
            dbms: &::ic_dbms_canister::prelude::WasmDbmsDatabase<'_, M>,
            table_name: &'static str,
            patch_values: &[(::ic_dbms_api::prelude::ColumnDef, ::ic_dbms_api::prelude::Value)],
            filter: Option<::ic_dbms_api::prelude::Filter>,
        ) -> ::ic_dbms_api::prelude::IcDbmsResult<u64> {
            use ::ic_dbms_api::prelude::TableSchema as _;
            use ::ic_dbms_api::prelude::UpdateRecord as _;
            use ::ic_dbms_api::prelude::Database as _;

            match table_name {
                #(#update_tables_arms)*
                _ => Err(::ic_dbms_api::prelude::IcDbmsError::Query(
                    ::ic_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    };

    let mut validate_insert_arms = vec![];
    for table in tables {
        let table_name = &table.table;
        validate_insert_arms.push(quote::quote! {
            name if name == #table_name::table_name() => {
                ::ic_dbms_canister::prelude::InsertIntegrityValidator::<#table_name, M>::new(dbms).validate(record_values)
            }
        });
    }

    let validate_insert_fn = quote::quote! {
        fn validate_insert(
            &self,
            dbms: &::ic_dbms_canister::prelude::WasmDbmsDatabase<'_, M>,
            table_name: &'static str,
            record_values: &[(::ic_dbms_api::prelude::ColumnDef, ::ic_dbms_api::prelude::Value)],
        ) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            use ::ic_dbms_api::prelude::TableSchema as _;
            match table_name {
                #(#validate_insert_arms)*
                _ => Err(::ic_dbms_api::prelude::IcDbmsError::Query(
                    ::ic_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    };

    let mut validate_update_arms = vec![];
    for table in tables {
        let table_name = &table.table;
        validate_update_arms.push(quote::quote! {
            name if name == #table_name::table_name() => {
                ::ic_dbms_canister::prelude::UpdateIntegrityValidator::<#table_name, M>::new(dbms, old_pk).validate(record_values)
            }
        });
    }

    let validate_update_fn = quote::quote! {
        fn validate_update(
            &self,
            dbms: &::ic_dbms_canister::prelude::WasmDbmsDatabase<'_, M>,
            table_name: &'static str,
            record_values: &[(::ic_dbms_api::prelude::ColumnDef, ::ic_dbms_api::prelude::Value)],
            old_pk: ::ic_dbms_api::prelude::Value,
        ) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            use ::ic_dbms_api::prelude::TableSchema as _;
            match table_name {
                #(#validate_update_arms)*
                _ => Err(::ic_dbms_api::prelude::IcDbmsError::Query(
                    ::ic_dbms_api::prelude::QueryError::TableNotFound(table_name.to_string()),
                )),
            }
        }
    };

    quote::quote! {
        impl<M> ::ic_dbms_canister::prelude::DatabaseSchema<M> for #struct_ident
        where
            M: ::ic_dbms_canister::prelude::MemoryProvider,
        {
            #select_fn
            #referenced_tables_fn
            #insert_tables_fn
            #delete_tables_fn
            #update_tables_fn
            #validate_insert_fn
            #validate_update_fn
        }
    }
}
