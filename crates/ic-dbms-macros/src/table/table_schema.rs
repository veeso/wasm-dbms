use proc_macro2::TokenStream as TokenStream2;
use syn::Ident;

use crate::table::metadata::{Field, Sanitizer, TableMetadata};

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
    let values = to_values(&metadata.fields);
    let sanitizers = sanitizers(&metadata.fields);
    let validators = validators(&metadata.fields);

    Ok(quote::quote! {
        impl ::ic_dbms_api::prelude::TableSchema for #struct_name {
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

            fn columns() -> &'static [::ic_dbms_api::prelude::ColumnDef] {
                #columns_def
            }

            fn to_values(self) -> Vec<(::ic_dbms_api::prelude::ColumnDef, ::ic_dbms_api::prelude::Value)> {
                #values
            }

            /// Returns the [`::ic_dbms_api::prelude::Sanitize`] implementation for the given column name, if any.
            fn sanitizer(column_name: &'static str) -> Option<Box<dyn ::ic_dbms_api::prelude::Sanitize>> {
                #sanitizers
            }

            /// Returns the [`::ic_dbms_api::prelude::Validate`] implementation for the given column name, if any.
            fn validator(column_name: &'static str) -> Option<Box<dyn ::ic_dbms_api::prelude::Validate>> {
                #validators
            }
        }
    })
}

fn column_def(metadata: &TableMetadata) -> syn::Result<TokenStream2> {
    let mut columns = vec![];

    for field in &metadata.fields {
        let primary_key = if field.primary_key {
            quote::quote! { true }
        } else {
            quote::quote! { false }
        };
        let name = &field.name.to_string();
        let foreign_key_def = foreign_key_def(field, metadata)?;
        let data_type_kind = &field.data_type_kind;
        let nullable = if field.nullable {
            quote::quote! { true }
        } else {
            quote::quote! { false }
        };

        columns.push(quote::quote! {
            ::ic_dbms_api::prelude::ColumnDef {
                data_type: #data_type_kind,
                foreign_key: #foreign_key_def,
                name: #name,
                nullable: #nullable,
                primary_key: #primary_key,
            }
        })
    }

    Ok(quote::quote! {
        &[#(#columns),*]
    })
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
        Some(::ic_dbms_api::prelude::ForeignKeyDef {
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
            // Custom type handling
            let field_type = &field.ty;
            if field.nullable {
                columns.push(quote::quote! {
                    (Self::columns()[#index], match #self_field {
                        ::ic_dbms_api::prelude::Nullable::Null => ::ic_dbms_api::prelude::Value::Null,
                        ::ic_dbms_api::prelude::Nullable::Value(ref inner) => {
                            ::ic_dbms_api::prelude::Value::Custom(::ic_dbms_api::prelude::CustomValue {
                                type_tag: <#field_type as ::ic_dbms_api::prelude::CustomDataType>::TYPE_TAG.to_string(),
                                encoded: ::ic_dbms_api::prelude::Encode::encode(inner).into_owned(),
                                display: ::std::string::ToString::to_string(inner),
                            })
                        }
                    })
                });
            } else {
                columns.push(quote::quote! {
                    (Self::columns()[#index], ::ic_dbms_api::prelude::Value::Custom(
                        ::ic_dbms_api::prelude::CustomValue {
                            type_tag: <#field_type as ::ic_dbms_api::prelude::CustomDataType>::TYPE_TAG.to_string(),
                            encoded: ::ic_dbms_api::prelude::Encode::encode(&#self_field).into_owned(),
                            display: ::std::string::ToString::to_string(&#self_field),
                        }
                    ))
                });
            }
        } else {
            // Built-in type handling
            let value_type = field.value_type.as_ref().expect("built-in field must have value_type");

            // For nullable we need to match whether it's Null.
            // If it's null we return `Value::Null`, otherwise we wrap the inner value.
            if field.nullable {
                columns.push(quote::quote! {
                    (Self::columns()[#index], match #self_field {
                        ::ic_dbms_api::prelude::Nullable::Null => ::ic_dbms_api::prelude::Value::Null,
                        ::ic_dbms_api::prelude::Nullable::Value(inner) => #value_type(inner),
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
