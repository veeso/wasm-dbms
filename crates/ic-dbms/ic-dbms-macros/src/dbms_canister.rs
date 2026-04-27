mod metadata;

use proc_macro2::TokenStream as TokenStream2;
use quote::format_ident;
use syn::DeriveInput;

use self::metadata::TableMetadata;

pub fn dbms_canister(input: DeriveInput) -> syn::Result<TokenStream2> {
    let metadata = self::metadata::collect_canister_metadata(&input.attrs)?;
    let struct_ident = &input.ident;

    let init_fn = impl_init(&metadata.tables);
    let inspect_fn = impl_inspect();
    let acl_api = impl_acl_api();
    let transaction_api = impl_transaction_api(struct_ident);
    let tables_api = impl_tables_api(&metadata.tables, struct_ident);
    let select_raw_api = impl_select_raw_api(struct_ident);
    let migration_api = impl_migration_api(struct_ident);

    Ok(quote::quote! {
        #init_fn
        #inspect_fn
        #acl_api
        #transaction_api
        #tables_api
        #select_raw_api
        #migration_api
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
                    if let Err(err) = ctx.acl_add(principal) {
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
        ) -> ::ic_dbms_api::prelude::IcDbmsResult<Vec<Vec<(::ic_dbms_api::prelude::JoinColumnDef, ::ic_dbms_api::prelude::Value)>>> {
            if query.has_joins() {
                ::ic_dbms_canister::api::select_join(&table, query, transaction_id, #struct_ident)
            } else {
                ::ic_dbms_canister::api::select_raw(&table, query, transaction_id, #struct_ident)
                    .map(|rows| {
                        rows.into_iter()
                            .map(|row| {
                                row.into_iter()
                                    .map(|(col, val)| (::ic_dbms_api::prelude::JoinColumnDef::from(col), val))
                                    .collect()
                            })
                            .collect()
                    })
            }
        }
    }
}

fn impl_migration_api(struct_ident: &syn::Ident) -> TokenStream2 {
    quote::quote! {
        #[::ic_cdk::query]
        fn has_drift() -> ::ic_dbms_api::prelude::IcDbmsResult<bool> {
            ::ic_dbms_canister::api::has_drift(#struct_ident)
        }

        #[::ic_cdk::query]
        fn pending_migrations() -> ::ic_dbms_api::prelude::IcDbmsResult<Vec<::ic_dbms_api::prelude::MigrationOp>> {
            ::ic_dbms_canister::api::pending_migrations(#struct_ident)
        }

        #[::ic_cdk::update]
        fn migrate(policy: ::ic_dbms_api::prelude::MigrationPolicy) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            ::ic_dbms_canister::api::migrate(policy, #struct_ident)
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
    let aggregate_fn_name = format_ident!("aggregate_{}", table_name);
    let insert_fn_name = format_ident!("insert_{}", table_name);
    let update_fn_name = format_ident!("update_{}", table_name);
    let delete_fn_name = format_ident!("delete_{}", table_name);

    quote::quote! {
        #[::ic_cdk::query]
        fn #select_fn_name(query: ::ic_dbms_api::prelude::Query, transaction_id: Option<::ic_dbms_api::prelude::TransactionId>) -> ::ic_dbms_api::prelude::IcDbmsResult<Vec<#record>> {
            ::ic_dbms_canister::api::select::<#entity, #struct_ident>(query, transaction_id, #struct_ident)
        }

        #[::ic_cdk::query]
        fn #aggregate_fn_name(
            query: ::ic_dbms_api::prelude::Query,
            aggregates: Vec<::ic_dbms_api::prelude::AggregateFunction>,
            transaction_id: Option<::ic_dbms_api::prelude::TransactionId>,
        ) -> ::ic_dbms_api::prelude::IcDbmsResult<Vec<::ic_dbms_api::prelude::AggregatedRow>> {
            ::ic_dbms_canister::api::aggregate::<#entity, #struct_ident>(query, aggregates, transaction_id, #struct_ident)
        }

        #[::ic_cdk::update]
        fn #insert_fn_name(record: #insert, transaction_id: Option<::ic_dbms_api::prelude::TransactionId>) -> ::ic_dbms_api::prelude::IcDbmsResult<()> {
            ::ic_dbms_canister::api::insert::<#entity, #struct_ident>(record, transaction_id, #struct_ident)
        }

        #[::ic_cdk::update]
        fn #update_fn_name(patch: #update, transaction_id: Option<::ic_dbms_api::prelude::TransactionId>) -> ::ic_dbms_api::prelude::IcDbmsResult<u64> {
            ::ic_dbms_canister::api::update::<#entity, #struct_ident>(patch, transaction_id, #struct_ident)
        }

        #[::ic_cdk::update]
        fn #delete_fn_name(delete_behavior: ::ic_dbms_api::prelude::DeleteBehavior, filter: Option<::ic_dbms_api::prelude::Filter>, transaction_id: Option<::ic_dbms_api::prelude::TransactionId>) -> ::ic_dbms_api::prelude::IcDbmsResult<u64> {
            ::ic_dbms_canister::api::delete::<#entity, #struct_ident>(delete_behavior, filter, transaction_id, #struct_ident)
        }
    }
}
