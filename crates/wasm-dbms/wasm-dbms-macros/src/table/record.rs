use proc_macro2::TokenStream as TokenStream2;
use syn::Ident;

use crate::table::metadata::TableMetadata;

/// Generate the `Record` implementation for `struct_name` using the provided `data` and `metadata`.
pub fn generate_record(struct_name: &Ident, metadata: &TableMetadata) -> TokenStream2 {
    let struct_def_tokens = struct_def(metadata);
    let impl_tokens = impl_record(struct_name, metadata);

    quote::quote! {
        #struct_def_tokens

        #impl_tokens
    }
}

/// Generate the `Struct` definition for the `Record` type using the provided `metadata`.
fn struct_def(metadata: &TableMetadata) -> TokenStream2 {
    let mut fields = vec![];

    for field in &metadata.fields {
        let name = &field.name;

        // if it is a fk, use the record type
        let fk = metadata
            .foreign_keys
            .iter()
            .find(|fk| fk.field == field.name);

        let ty = if let Some(fk) = fk {
            let entity_record = &fk.record_type;
            // if nullable, wrap in Nullable
            if field.nullable {
                quote::quote! {
                    ::wasm_dbms_api::prelude::Nullable<Box<#entity_record>>
                }
            } else {
                quote::quote! { #entity_record }
            }
        } else {
            let value_ty = &field.ty;
            quote::quote! { #value_ty }
        };

        if field.is_fk {
            fields.push(quote::quote! {
                pub #name: Option<Box<#ty>>,
            });
        } else {
            fields.push(quote::quote! {
                pub #name: Option<#ty>,
            });
        }
    }

    let record_ident = &metadata.record;

    quote::quote! {
        #[derive(Debug, Clone, Eq, PartialEq, candid::CandidType, serde::Serialize, serde::Deserialize)]
        pub struct #record_ident {
            #(#fields)*
        }
    }
}

fn impl_record(struct_name: &Ident, metadata: &TableMetadata) -> TokenStream2 {
    let impl_for = &metadata.record;
    let from_values_impl = impl_from_values(metadata);
    let to_values_impl = impl_to_values(metadata);

    quote::quote! {
        impl ::wasm_dbms_api::prelude::TableRecord for #impl_for {
            type Schema = #struct_name;

            #from_values_impl

            #to_values_impl
        }
    }
}

fn impl_from_values(metadata: &TableMetadata) -> TokenStream2 {
    // declare all `let field = None;` for each field
    let mut field_inits = vec![];
    for field in &metadata.fields {
        let field_name = &field.name;
        if field.is_fk {
            // use entity record type
            let fk = metadata
                .foreign_keys
                .iter()
                .find(|fk| fk.field == field.name)
                .expect("Foreign key metadata should exist for foreign key field");
            let entity_record = &fk.record_type;
            field_inits.push(quote::quote! {
                let mut #field_name: Option<Box<#entity_record>> = None;
            });
        } else {
            let field_ty = &field.ty;
            field_inits.push(quote::quote! {
                let mut #field_name: Option<#field_ty> = None;
            });
        }
    }

    let mut struct_fields = vec![];
    for field in &metadata.fields {
        let field_name = &field.name;
        struct_fields.push(quote::quote! {
            #field_name,
        });
    }

    // make match for each column (except fk)
    let mut field_matches = vec![];
    for field in metadata.fields.iter().filter(|f| !f.is_fk) {
        let field_ident = &field.name;
        let field_name = field.name.to_string();

        if field.custom_type {
            let custom_ident = field
                .custom_type_ident
                .as_ref()
                .expect("custom_type field must have custom_type_ident");
            if field.nullable {
                field_matches.push(quote::quote! {
                    #field_name => {
                        if let ::wasm_dbms_api::prelude::Value::Custom(cv) = value {
                            if let Ok(decoded) = <#custom_ident as ::wasm_dbms_api::prelude::Encode>::decode(
                                std::borrow::Cow::Borrowed(&cv.encoded)
                            ) {
                                #field_ident = Some(::wasm_dbms_api::prelude::Nullable::Value(decoded));
                            }
                        } else if let ::wasm_dbms_api::prelude::Value::Null = value {
                            #field_ident = Some(::wasm_dbms_api::prelude::Nullable::Null);
                        }
                    }
                });
            } else {
                field_matches.push(quote::quote! {
                    #field_name => {
                        if let ::wasm_dbms_api::prelude::Value::Custom(cv) = value {
                            if let Ok(decoded) = <#custom_ident as ::wasm_dbms_api::prelude::Encode>::decode(
                                std::borrow::Cow::Borrowed(&cv.encoded)
                            ) {
                                #field_ident = Some(decoded);
                            }
                        }
                    }
                });
            }
        } else {
            let value_type = field
                .value_type
                .as_ref()
                .expect("built-in field must have value_type");

            // if is nullable, behaviour is different
            if field.nullable {
                field_matches.push(quote::quote! {
                    #field_name => {
                        if let #value_type(value) = value {
                            #field_ident = Some(::wasm_dbms_api::prelude::Nullable::Value(value.clone()));
                        } else if let ::wasm_dbms_api::prelude::Value::Null = value {
                            #field_ident = Some(::wasm_dbms_api::prelude::Nullable::Null);
                        }
                    }
                });
            } else if field.is_fk {
                field_matches.push(quote::quote! {
                    #field_name => {
                        if let #value_type(value) = value {
                            #field_ident = Some(Box::new(value.clone()));
                        }
                    }
                });
            } else {
                field_matches.push(quote::quote! {
                    #field_name => {
                        if let #value_type(value) = value {
                            #field_ident = Some(value.clone());
                        }
                    }
                });
            }
        }
    }

    // make fk blocks
    let mut fk_matches = vec![];
    for fk in &metadata.foreign_keys {
        let table_name = fk.referenced_table.to_string();
        let local_column = fk.field.to_string();
        let fk_entity_record = &fk.record_type;
        // make path for fk_entity_record::from_values
        let fk_from_record_path = quote::quote! {
            #fk_entity_record::from_values
        };
        let field_name = &fk.field;

        fk_matches.push(quote::quote! {
            let has_fk_values = values.iter().any(|(source, _)| {
                *source ==
                    ::wasm_dbms_api::prelude::ValuesSource::Foreign {
                        table: #table_name.to_string(),
                        column: #local_column.to_string(),
                    }
            });


            if has_fk_values {
                #field_name = Some(Box::new(
                    #fk_from_record_path(
                        ::wasm_dbms_api::prelude::self_reference_values(
                            &values,
                            #table_name,
                            #local_column,
                        )
                    )
                ));
            }
        })
    }

    quote::quote! {
        #[allow(clippy::copy_clone)]
        fn from_values(values: ::wasm_dbms_api::prelude::TableColumns) -> Self {
            #(#field_inits)*

            let this_record_values = values
                .iter()
                .find(|(table_name, _)| *table_name == ::wasm_dbms_api::prelude::ValuesSource::This)
                .map(|(_, cols)| cols);

            for (column, value) in this_record_values.unwrap_or(&vec![]) {
                match column.name {
                    #(#field_matches)*
                    _ => {} // ignore unknown/fk columns
                }
            }

            #(#fk_matches)*

            Self {
                #(#struct_fields)*
            }
        }
    }
}

fn impl_to_values(metadata: &TableMetadata) -> TokenStream2 {
    let mut field_match = vec![];

    for field in &metadata.fields {
        let field_name = &field.name;
        let self_field_name = quote::quote! { &self.#field_name };

        if field.custom_type {
            let custom_ident = field
                .custom_type_ident
                .as_ref()
                .expect("custom_type field must have custom_type_ident");
            if field.nullable {
                field_match.push(quote::quote! {
                    match #self_field_name {
                        Some(::wasm_dbms_api::prelude::Nullable::Value(value)) => {
                            ::wasm_dbms_api::prelude::Value::Custom(
                                ::wasm_dbms_api::prelude::CustomValue::new::<#custom_ident>(value)
                            )
                        }
                        Some(::wasm_dbms_api::prelude::Nullable::Null) | None => ::wasm_dbms_api::prelude::Value::Null,
                    }
                });
            } else if field.is_fk {
                continue;
            } else {
                field_match.push(quote::quote! {
                    match #self_field_name {
                        Some(value) => ::wasm_dbms_api::prelude::Value::Custom(
                            ::wasm_dbms_api::prelude::CustomValue::new::<#custom_ident>(value)
                        ),
                        None => ::wasm_dbms_api::prelude::Value::Null,
                    }
                });
            }
        } else {
            let value_type = field
                .value_type
                .as_ref()
                .expect("built-in field must have value_type");

            // handle nullable
            if field.nullable {
                field_match.push(quote::quote! {
                    match #self_field_name {
                        Some(::wasm_dbms_api::prelude::Nullable::Value(value)) => #value_type(value.clone()),
                        Some(::wasm_dbms_api::prelude::Nullable::Null) | None => ::wasm_dbms_api::prelude::Value::Null,
                    }
                });
            } else if field.is_fk {
                // do not push fk fields
                continue;
            } else {
                field_match.push(quote::quote! {
                    match #self_field_name {
                        Some(value) => #value_type(value.clone()),
                        None => ::wasm_dbms_api::prelude::Value::Null,
                    }
                });
            }
        }
    }

    quote::quote! {
        #[allow(clippy::copy_clone)]
        fn to_values(&self) -> Vec<(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)> {
            use ::wasm_dbms_api::prelude::TableSchema as _;

            Self::Schema::columns()
                .iter()
                .filter(|col| col.foreign_key.is_none())
                .zip(vec![
                    #(#field_match),*
                ])
                .map(|(col_def, value)| (*col_def, value))
                .collect()
        }
    }
}
