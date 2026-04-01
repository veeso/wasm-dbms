use proc_macro2::TokenStream as TokenStream2;
use syn::Ident;

use crate::table::metadata::TableMetadata;

pub fn generate_insert_request(struct_name: &Ident, metadata: &TableMetadata) -> TokenStream2 {
    let insert_request_struct = generate_insert_request_struct(metadata);
    let insert_record_impl = impl_insert_record(struct_name, metadata);

    quote::quote! {
        #insert_request_struct
        #insert_record_impl
    }
}

/// Expected to generate for:
///
/// ```rust,ignore
/// pub struct Post {
///    pub id: Uint32,
///    pub title: Text,
///    pub content: Text,
///    pub user_id: Uint32,
///}
/// ```
///
/// ```rust,ignore
/// #[derive(Clone)]
/// pub struct PostInsertRequest {
///     pub id: Uint32,
///     pub title: Text,
///     pub content: Text,
///     pub user_id: Uint32,
/// }
/// ```
fn generate_insert_request_struct(metadata: &TableMetadata) -> TokenStream2 {
    let mut fields = vec![];

    for field in &metadata.fields {
        let name = &field.name;
        let value_ty = &field.ty;
        if field.auto_increment {
            fields.push(quote::quote! {
                pub #name: ::wasm_dbms_api::prelude::Autoincrement<#value_ty>,
            });
        } else {
            fields.push(quote::quote! {
                pub #name: #value_ty,
            });
        }
    }

    let insert_request_ident = &metadata.insert;

    let derives = if metadata.candid {
        quote::quote! {
            #[derive(Clone, candid::CandidType, serde::Serialize, serde::Deserialize)]
        }
    } else {
        quote::quote! {
            #[derive(Clone)]
        }
    };

    quote::quote! {
        #derives
        pub struct #insert_request_ident {
            #(#fields)*
        }
    }
}

fn impl_insert_record(struct_name: &Ident, metadata: &TableMetadata) -> TokenStream2 {
    let insert_request_ident = &metadata.insert;
    let record_ident = &metadata.record;

    let from_values_impl = impl_from_values(metadata);
    let into_values_impl = impl_into_values(metadata);
    let into_record_impl = impl_into_record(metadata);

    quote::quote! {
        impl ::wasm_dbms_api::prelude::InsertRecord for #insert_request_ident {
            type Record = #record_ident;
            type Schema = #struct_name;

            #from_values_impl
            #into_values_impl
            #into_record_impl
        }
    }
}

/// Expected to generate for:
///
/// ```rust,ignore
/// pub struct PostInsertRequest {
///     pub id: Uint32,
///     pub title: Text,
///     pub content: Text,
///     pub user_id: Uint32,
/// }
/// ```
///
/// ```rust,ignore
///fn from_values(values: &[(ColumnDef, Value)]) -> wasm_dbms_api::prelude::DbmsResult<Self> {
///    let mut id: Option<Uint32> = None;
///    let mut title: Option<Text> = None;
///    let mut content: Option<Text> = None;
///    let mut user_id: Option<Uint32> = None;
///    for (column, value) in values {
///        match column.name {
///            "id" => {
///                if let Value::Uint32(v) = value {
///                    id = Some(*v);
///                }
///            }
///            "title" => {
///                if let Value::Text(v) = value {
///                    title = Some(v.clone());
///                }
///            }
///            "content" => {
///                if let Value::Text(v) = value {
///                    content = Some(v.clone());
///                }
///            }
///            "user_id" => {
///                if let Value::Uint32(v) = value {
///                    user_id = Some(*v);
///                }
///            }
///            _ => { /* Ignore unknown columns */ }
///        }
///    }
///    Ok(Self {
///        id: id.ok_or(DbmsError::Query(QueryError::MissingNonNullableField(
///            "id".to_string(),
///        )))?,
///        title: title.ok_or(DbmsError::Query(QueryError::MissingNonNullableField(
///            "title".to_string(),
///        )))?,
///        content: content.ok_or(DbmsError::Query(QueryError::MissingNonNullableField(
///            "content".to_string(),
///        )))?,
///        user_id: user_id.ok_or(DbmsError::Query(QueryError::MissingNonNullableField(
///            "user_id".to_string(),
///        )))?,
///    })
///}
/// ```
fn impl_from_values(metadata: &TableMetadata) -> TokenStream2 {
    let mut declare_lets = vec![];
    for field in &metadata.fields {
        let name = &field.name;
        let ty = &field.ty;

        // autoincrement fields store the inner type in the intermediate variable
        declare_lets.push(quote::quote! {
            let mut #name: Option<#ty> = None;
        });
    }

    let mut match_arms = vec![];
    for field in &metadata.fields {
        let field_name = &field.name;
        let field_name_str = field.name.to_string();

        if field.custom_type {
            let custom_ident = field
                .custom_type_ident
                .as_ref()
                .expect("custom_type field must have custom_type_ident");
            if field.nullable {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let ::wasm_dbms_api::prelude::Value::Custom(cv) = __col_value {
                            if let Ok(decoded) = <#custom_ident as ::wasm_dbms_api::prelude::Encode>::decode(
                                std::borrow::Cow::Borrowed(&cv.encoded)
                            ) {
                                #field_name = Some(::wasm_dbms_api::prelude::Nullable::Value(decoded));
                            }
                        } else if let ::wasm_dbms_api::prelude::Value::Null = __col_value {
                            #field_name = Some(::wasm_dbms_api::prelude::Nullable::Null);
                        }
                    }
                });
            } else {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let ::wasm_dbms_api::prelude::Value::Custom(cv) = __col_value {
                            if let Ok(decoded) = <#custom_ident as ::wasm_dbms_api::prelude::Encode>::decode(
                                std::borrow::Cow::Borrowed(&cv.encoded)
                            ) {
                                #field_name = Some(decoded);
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

            if field.nullable {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let #value_type(__inner_value) = __col_value {
                            #field_name = Some(::wasm_dbms_api::prelude::Nullable::Value(__inner_value.clone()));
                        } else if let ::wasm_dbms_api::prelude::Value::Null = __col_value {
                            #field_name = Some(::wasm_dbms_api::prelude::Nullable::Null);
                        }
                    }
                });
            } else {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let #value_type(__inner_value) = __col_value {
                            #field_name = Some(__inner_value.clone());
                        }
                    }
                });
            }
        }
    }

    let mut struct_fields = vec![];
    for field in &metadata.fields {
        let name = &field.name;
        let name_str = name.to_string();

        if field.auto_increment {
            // autoincrement: wrap in Autoincrement::Value if present, Autoincrement::Auto if absent
            struct_fields.push(quote::quote! {
                #name: match #name {
                    Some(v) => ::wasm_dbms_api::prelude::Autoincrement::Value(v),
                    None => ::wasm_dbms_api::prelude::Autoincrement::Auto,
                },
            });
        } else if field.nullable {
            struct_fields.push(quote::quote! {
                #name: #name.unwrap_or(::wasm_dbms_api::prelude::Nullable::Null),
            })
        } else {
            struct_fields.push(quote::quote! {
                #name: #name.ok_or(::wasm_dbms_api::prelude::DbmsError::Query(::wasm_dbms_api::prelude::QueryError::MissingNonNullableField(
                    #name_str.to_string(),
                )))?,
            })
        }
    }

    quote::quote! {
        #[allow(clippy::copy_clone)]
        fn from_values(values: &[(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)]) -> ::wasm_dbms_api::prelude::DbmsResult<Self> {
            #(#declare_lets)*

            for (column, __col_value) in values {
                match column.name {
                    #(#match_arms)*
                    _ => { /* Ignore unknown columns */ }
                }
            }

            Ok(Self {
                #(#struct_fields)*
            })
        }
    }
}

/// Expected to generate for:
///
/// ```rust,ignore
/// pub struct PostInsertRequest {
///     pub id: Uint32,
///     pub title: Text,
///     pub content: Text,
///     pub user_id: Uint32,
/// }
/// ```
/// ```rust,ignore
/// fn into_values(self) -> Vec<(ColumnDef, Value)> {
///     vec![
///         (Self::Schema::columns()[0], Value::Uint32(self.id)),
///         (Self::Schema::columns()[1], Value::Text(self.title)),
///         (Self::Schema::columns()[2], Value::Text(self.content)),
///         (Self::Schema::columns()[3], Value::Uint32(self.user_id)),
///     ]
/// }
/// ```
fn impl_into_values(metadata: &TableMetadata) -> TokenStream2 {
    let mut push_stmts = vec![];
    for (index, field) in metadata.fields.iter().enumerate() {
        let field_name = &field.name;
        if field.auto_increment {
            push_stmts.push(quote::quote! {
                if let ::wasm_dbms_api::prelude::Autoincrement::Value(v) = self.#field_name {
                    values.push((Self::Schema::columns()[#index], v.into()));
                }
            });
        } else {
            push_stmts.push(quote::quote! {
                values.push((Self::Schema::columns()[#index], self.#field_name.into()));
            });
        }
    }

    quote::quote! {
        fn into_values(self) -> Vec<(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)> {
            use ::wasm_dbms_api::prelude::TableSchema as _;

            let mut values = Vec::new();
            #(#push_stmts)*
            values
        }
    }
}

/// Expected to generate for:
///
/// ```rust,ignore
/// pub struct PostInsertRequest {
///     pub id: Uint32,
///     pub title: Text,
///     pub content: Text,
///     pub user_id: Uint32,
/// }
/// ```
///
/// ```rust,ignore
/// fn into_record(self) -> Self::Schema {
///     Post {
///         id: self.id,
///         title: self.title,
///         content: self.content,
///         user_id: self.user_id,
///     }
/// }
/// ```
fn impl_into_record(metadata: &TableMetadata) -> TokenStream2 {
    let mut fields = vec![];
    for field in &metadata.fields {
        let name = &field.name;
        if field.auto_increment {
            // unwrap Autoincrement::Value -> T; panic on Auto since values must be resolved by now
            let name_str = name.to_string();
            fields.push(quote::quote! {
                #name: match self.#name {
                    ::wasm_dbms_api::prelude::Autoincrement::Value(v) => v,
                    ::wasm_dbms_api::prelude::Autoincrement::Auto => panic!(
                        "autoincrement field '{}' was not resolved before into_record()", #name_str
                    ),
                },
            });
        } else {
            fields.push(quote::quote! {
                #name: self.#name,
            });
        }
    }

    quote::quote! {
        fn into_record(self) -> Self::Schema {
            Self::Schema {
                #(#fields)*
            }
        }
    }
}
