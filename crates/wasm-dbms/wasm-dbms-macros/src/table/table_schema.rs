use proc_macro2::TokenStream as TokenStream2;
use syn::Ident;

use crate::table::metadata::{Field, Index, Sanitizer, TableMetadata};

/// Generate the table schema implementation for `struct_name` using the provided `data` and `metadata`.
pub fn generate_table_schema(
    struct_name: &Ident,
    metadata: &TableMetadata,
) -> syn::Result<TokenStream2> {
    let record_ident = metadata.record.clone();
    let insert_ident = metadata.insert.clone();
    let update_ident = metadata.update.clone();
    let primary_key = metadata.primary_key.clone();
    let foreign_fetcher_ident = metadata.foreign_fetcher_ident();
    let table_name = metadata.name.to_string();
    let primary_key_str = primary_key.to_string();
    let columns_def = column_def(metadata)?;
    let indexes_def = indexes_def(&metadata.indexes);
    let values = to_values(&metadata.fields);
    let sanitizers = sanitizers(&metadata.fields);
    let validators = validators(&metadata.fields);
    let migrate_impl = migrate_impl(struct_name, metadata);

    Ok(quote::quote! {
        #migrate_impl

        impl ::wasm_dbms_api::prelude::TableSchema for #struct_name {
            type Record = #record_ident;
            type Insert = #insert_ident;
            type Update = #update_ident;
            type ForeignFetcher = #foreign_fetcher_ident;

            fn table_name() -> &'static str {
                #table_name
            }

            fn primary_key() -> &'static str {
                #primary_key_str
            }

            fn columns() -> &'static [::wasm_dbms_api::prelude::ColumnDef] {
                #columns_def
            }

            fn indexes() -> &'static [::wasm_dbms_api::prelude::IndexDef] {
                #indexes_def
            }

            fn to_values(self) -> Vec<(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)> {
                #values
            }

            /// Returns the [`::wasm_dbms_api::prelude::Sanitize`] implementation for the given column name, if any.
            fn sanitizer(column_name: &'static str) -> Option<Box<dyn ::wasm_dbms_api::prelude::Sanitize>> {
                #sanitizers
            }

            /// Returns the [`::wasm_dbms_api::prelude::Validate`] implementation for the given column name, if any.
            fn validator(column_name: &'static str) -> Option<Box<dyn ::wasm_dbms_api::prelude::Validate>> {
                #validators
            }
        }
    })
}

/// Generate the static `&[IndexDef]` slice for the `indexes()` method.
fn indexes_def(indexes: &[Index]) -> TokenStream2 {
    let entries: Vec<_> = indexes
        .iter()
        .map(|index| {
            let col_strs: Vec<_> = index.columns.iter().map(|c| c.to_string()).collect();
            quote::quote! {
                ::wasm_dbms_api::prelude::IndexDef(&[#(#col_strs),*])
            }
        })
        .collect();

    quote::quote! {
        &[#(#entries),*]
    }
}

fn column_def(metadata: &TableMetadata) -> syn::Result<TokenStream2> {
    let mut columns = vec![];

    for field in &metadata.fields {
        let primary_key = quote_bool(field.primary_key);
        let name = &field.name.to_string();
        let foreign_key_def = foreign_key_def(field, metadata)?;
        let data_type_kind = &field.data_type_kind;
        let nullable = quote_bool(field.nullable);
        // set unique to true if either the field is marked as unique or it's a primary key (since primary keys are implicitly unique)
        let unique = quote_bool(field.unique || field.primary_key);
        let auto_increment = quote_bool(field.auto_increment);
        let default = default_expr(field);
        let renamed_from = renamed_from_expr(field);

        columns.push(quote::quote! {
            ::wasm_dbms_api::prelude::ColumnDef {
                data_type: #data_type_kind,
                foreign_key: #foreign_key_def,
                name: #name,
                auto_increment: #auto_increment,
                nullable: #nullable,
                unique: #unique,
                primary_key: #primary_key,
                default: #default,
                renamed_from: #renamed_from,
            }
        })
    }

    Ok(quote::quote! {
        {
            const COLUMNS: &[::wasm_dbms_api::prelude::ColumnDef] = &[#(#columns),*];
            COLUMNS
        }
    })
}

/// Build the `default` field expression for a generated `ColumnDef` literal.
///
/// Returns either `None` (no `#[default]` attribute) or
/// `Some((|| ::wasm_dbms_api::prelude::Value::from(<inner>::from(<expr>))) as fn() -> Value)`
/// for built-in types (so an unsuffixed integer literal coerces into the
/// column's specific `Value` variant rather than defaulting to `i32`), and
/// `Some((|| Value::from(<expr>)) as fn() -> Value)` for custom types where
/// `From<CustomType> for Value` is provided by the `#[derive(CustomDataType)]`
/// macro.
///
/// The fn-pointer cast keeps [`ColumnDef`](::wasm_dbms_api::prelude::ColumnDef)
/// `Copy`.
fn default_expr(field: &Field) -> TokenStream2 {
    let Some(expr) = field.default.as_ref() else {
        return quote::quote! { ::core::option::Option::None };
    };

    let body = if field.custom_type {
        quote::quote! {
            ::wasm_dbms_api::prelude::Value::from(#expr)
        }
    } else {
        // value_type is set for non-custom fields (e.g. `Value::Uint32`); the
        // last segment names the inner type (e.g. `Uint32`).
        let value_type = field
            .value_type
            .as_ref()
            .expect("non-custom field must carry a value_type");
        let inner_ident = &value_type
            .segments
            .last()
            .expect("value_type path must have at least one segment")
            .ident;
        quote::quote! {
            ::wasm_dbms_api::prelude::Value::from(
                <::wasm_dbms_api::prelude::#inner_ident as ::core::convert::From<_>>::from(#expr)
            )
        }
    };

    quote::quote! {
        ::core::option::Option::Some(
            (|| #body) as fn() -> ::wasm_dbms_api::prelude::Value
        )
    }
}

/// Build the `renamed_from` field expression for a generated `ColumnDef` literal.
fn renamed_from_expr(field: &Field) -> TokenStream2 {
    let entries = field.renamed_from.iter().map(|s| quote::quote! { #s });
    quote::quote! {
        &[#(#entries),*]
    }
}

/// Emit `impl Migrate for #struct_name {}` unless the struct carries
/// `#[migrate]`, in which case the user is expected to provide their own
/// implementation.
fn migrate_impl(struct_name: &Ident, metadata: &TableMetadata) -> TokenStream2 {
    if metadata.user_migrate_impl {
        quote::quote! {}
    } else {
        quote::quote! {
            impl ::wasm_dbms_api::prelude::Migrate for #struct_name {}
        }
    }
}

/// Build up the `ForeignKeyDef` definition for the given field, if it is a foreign key.
fn foreign_key_def(field: &Field, metadata: &TableMetadata) -> syn::Result<TokenStream2> {
    let Some(foreign_key_for_field) = metadata
        .foreign_keys
        .iter()
        .find(|fk| fk.field == field.name)
    else {
        return Ok(quote::quote! { None });
    };

    let local_column = foreign_key_for_field.field.to_string();
    let foreign_table = foreign_key_for_field.referenced_table.to_string();
    let foreign_column = foreign_key_for_field.referenced_field.to_string();

    Ok(quote::quote! {
        Some(::wasm_dbms_api::prelude::ForeignKeyDef {
            local_column: #local_column,
            foreign_table: #foreign_table,
            foreign_column: #foreign_column,
        })
    })
}

fn to_values(fields: &[Field]) -> TokenStream2 {
    let mut columns = vec![];

    for (index, field) in fields.iter().enumerate() {
        let field_ident = &field.name;
        let self_field: syn::Expr = syn::parse_quote! {
            self.#field_ident
        };

        if field.custom_type {
            // Custom type handling -- use the inner type ident (Nullable stripped) for trait lookups
            let custom_ident = field
                .custom_type_ident
                .as_ref()
                .expect("custom_type field must have custom_type_ident");
            if field.nullable {
                columns.push(quote::quote! {
                    (Self::columns()[#index], match #self_field {
                        ::wasm_dbms_api::prelude::Nullable::Null => ::wasm_dbms_api::prelude::Value::Null,
                        ::wasm_dbms_api::prelude::Nullable::Value(inner) => {
                            ::wasm_dbms_api::prelude::Value::Custom(::wasm_dbms_api::prelude::CustomValue {
                                type_tag: <#custom_ident as ::wasm_dbms_api::prelude::CustomDataType>::TYPE_TAG.to_string(),
                                encoded: ::wasm_dbms_api::prelude::Encode::encode(&inner).into_owned(),
                                display: ::std::string::ToString::to_string(&inner),
                            })
                        }
                    })
                });
            } else {
                columns.push(quote::quote! {
                    (Self::columns()[#index], ::wasm_dbms_api::prelude::Value::Custom(
                        ::wasm_dbms_api::prelude::CustomValue {
                            type_tag: <#custom_ident as ::wasm_dbms_api::prelude::CustomDataType>::TYPE_TAG.to_string(),
                            encoded: ::wasm_dbms_api::prelude::Encode::encode(&#self_field).into_owned(),
                            display: ::std::string::ToString::to_string(&#self_field),
                        }
                    ))
                });
            }
        } else {
            // Built-in type handling
            let value_type = field
                .value_type
                .as_ref()
                .expect("built-in field must have value_type");

            // For nullable we need to match whether it's Null.
            // If it's null we return `Value::Null`, otherwise we wrap the inner value.
            if field.nullable {
                columns.push(quote::quote! {
                    (Self::columns()[#index], match #self_field {
                        ::wasm_dbms_api::prelude::Nullable::Null => ::wasm_dbms_api::prelude::Value::Null,
                        ::wasm_dbms_api::prelude::Nullable::Value(inner) => #value_type(inner),
                    })
                });
            } else {
                columns.push(quote::quote! {
                    (Self::columns()[#index], #value_type(#self_field))
                });
            }
        }
    }

    quote::quote! {
        vec![#(#columns),*]
    }
}

/// Generate the match arms for the validators function.
fn validators(fields: &[Field]) -> TokenStream2 {
    let mut arms = vec![];

    for field in fields {
        if let Some(validator) = &field.validate {
            let field_name = field.name.to_string();
            let validator_struct = &validator.path;
            let args = &validator.args;
            if args.is_empty() {
                arms.push(quote::quote! {
                    #field_name => Some(Box::new(#validator_struct)),
                });
            } else {
                arms.push(quote::quote! {
                    #field_name => Some(Box::new(#validator_struct(#(#args),*))),
                });
            }
        }
    }

    arms.push(quote::quote! {
        _ => None,
    });

    quote::quote! {
        match column_name {
            #(#arms)*
        }
    }
}

/// Generate the match arms for the sanitizers function.
fn sanitizers(fields: &[Field]) -> TokenStream2 {
    let mut arms = vec![];

    for field in fields {
        if let Some(sanitizer) = &field.sanitize {
            let field_name = field.name.to_string();
            match sanitizer {
                Sanitizer::Unit { name } => {
                    arms.push(quote::quote! {
                        #field_name => Some(Box::new(#name)),
                    });
                }
                Sanitizer::Tuple { name, args } => {
                    arms.push(quote::quote! {
                        #field_name => Some(Box::new(#name(#(#args),*))),
                    });
                }
                Sanitizer::NamedArgs { name, args } => {
                    let fields = args.iter().map(|(ident, expr)| {
                        quote::quote! {
                            #ident: #expr
                        }
                    });
                    arms.push(quote::quote! {
                        #field_name => Some(Box::new(#name { #(#fields),* })),
                    });
                }
            }
        }
    }

    arms.push(quote::quote! {
        _ => None,
    });

    quote::quote! {
        match column_name {
            #(#arms)*
        }
    }
}

fn quote_bool(value: bool) -> TokenStream2 {
    if value {
        quote::quote! { true }
    } else {
        quote::quote! { false }
    }
}
