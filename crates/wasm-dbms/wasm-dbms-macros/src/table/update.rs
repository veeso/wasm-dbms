use proc_macro2::TokenStream as TokenStream2;
use syn::Ident;

use crate::table::metadata::TableMetadata;

/// Generate the update request type for `struct_name`, declared with `vis`.
pub fn generate_update_request(
    struct_name: &Ident,
    vis: &syn::Visibility,
    generics: &syn::Generics,
    metadata: &TableMetadata,
) -> TokenStream2 {
    let update_request_struct = generate_update_request_struct(vis, generics, metadata);
    let update_record_impl = impl_update_record(struct_name, generics, metadata);

    quote::quote! {
        #update_request_struct
        #update_record_impl
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
/// pub struct PostUpdateRequest {
///     pub id: Option<Uint32>,
///     pub title: Option<Text>,
///     pub content: Option<Text>,
///     pub user_id: Option<Uint32>,
///     pub where_clause: Option<Filter>,
/// }
/// ```
fn generate_update_request_struct(
    vis: &syn::Visibility,
    generics: &syn::Generics,
    metadata: &TableMetadata,
) -> TokenStream2 {
    let mut fields = vec![];

    for field in &metadata.fields {
        let name = &field.name;
        let value_ty = &field.ty;
        fields.push(quote::quote! {
            pub #name: Option<#value_ty>,
        });
    }

    let update_request_ident = &metadata.update;
    let where_clause = &generics.where_clause;

    let derives = if metadata.candid {
        quote::quote! {
            #[derive(Clone, Default, candid::CandidType, serde::Serialize, serde::Deserialize)]
        }
    } else {
        quote::quote! {
            #[derive(Clone, Default)]
        }
    };

    quote::quote! {
        #derives
        #vis struct #update_request_ident #generics #where_clause {
            #(#fields)*
            pub where_clause: Option<::wasm_dbms_api::prelude::Filter>,
        }
    }
}

fn impl_update_record(
    struct_name: &Ident,
    generics: &syn::Generics,
    metadata: &TableMetadata,
) -> TokenStream2 {
    let update_request_ident = &metadata.update;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let record_ident = &metadata.record;

    let from_values_impl = impl_from_values(metadata);
    let into_values_impl = impl_update_values(metadata);

    quote::quote! {
        impl #impl_generics ::wasm_dbms_api::prelude::UpdateRecord for #update_request_ident #ty_generics #where_clause {
            type Record = #record_ident #ty_generics;
            type Schema = #struct_name #ty_generics;

            #from_values_impl
            #into_values_impl

            fn where_clause(&self) -> Option<::wasm_dbms_api::prelude::Filter> {
                self.where_clause.clone()
            }
        }
    }
}

/// Expected to generate for
///
/// ```rust,ignore
/// pub struct PostUpdateRequest {
///     pub id: Option<Uint32>,
///     pub title: Option<Text>,
///     pub content: Option<Text>,
///     pub user_id: Option<Uint32>,
///     pub where_clause: Option<Filter>,
/// }
/// ```
///
/// ```rust,ignore
/// fn from_values(values: &[(ColumnDef, Value)], filter: Option<Filter>) -> DbmsResult<Self> {
///    let mut id: Option<Uint32> = None;
///    let mut title: Option<Text> = None;
///    let mut content: Option<Text> = None;
///    let mut user_id: Option<Uint32> = None;
///
///    for (column, value) in values {
///        match column.name {
///            "id" => {
///                id = Some(match value {
///                    Value::Uint32(v) => v.clone(),
///                    _ => return Err(/* QueryError::InvalidQuery: wrong value type */),
///                });
///            }
///            // ... same for `title`, `content` and `user_id`; nullable columns also
///            // accept `Value::Null`, custom columns accept a `Value::Custom` with their tag
///            _ => { /* Ignore unknown columns */ }
///        }
///    }
///
///    Ok(Self {
///        id,
///        title,
///        content,
///        user_id,
///        where_clause: filter,
///    })
///}
/// ```
fn impl_from_values(metadata: &TableMetadata) -> TokenStream2 {
    let mut field_initializers = vec![];
    for field in &metadata.fields {
        let field_name = &field.name;
        let field_type = &field.ty;
        field_initializers.push(quote::quote! {
            let mut #field_name: Option<#field_type> = None;
        });
    }

    let mut match_arms = vec![];
    for field in &metadata.fields {
        let field_name = &field.name;
        let field_name_str = field.name.to_string();
        let mismatch = quote::quote! {
            return Err(::wasm_dbms_api::prelude::DbmsError::Query(
                ::wasm_dbms_api::prelude::QueryError::InvalidQuery(format!(
                    "column '{column}' does not accept a value of type {value_type}",
                    column = #field_name_str,
                    value_type = __col_value.type_name(),
                )),
            ))
        };

        // the value accepted for the column, converted to the field type
        let accepted = if field.custom_type {
            let custom_ident = field
                .custom_type_path
                .as_ref()
                .expect("custom_type field must have custom_type_path");
            let decoded = quote::quote! {
                match <#custom_ident as ::wasm_dbms_api::prelude::Encode>::decode(
                    std::borrow::Cow::Borrowed(&__wasm_dbms_custom.encoded)
                ) {
                    Ok(__wasm_dbms_decoded) => __wasm_dbms_decoded,
                    Err(_) => #mismatch,
                }
            };
            let decoded = if field.nullable {
                quote::quote! { ::wasm_dbms_api::prelude::Nullable::Value(#decoded) }
            } else {
                decoded
            };
            quote::quote! {
                ::wasm_dbms_api::prelude::Value::Custom(__wasm_dbms_custom)
                    if __wasm_dbms_custom.type_tag
                        == <#custom_ident as ::wasm_dbms_api::prelude::CustomDataType>::TYPE_TAG =>
                {
                    #decoded
                }
            }
        } else {
            let value_type = field
                .value_type
                .as_ref()
                .expect("built-in field must have value_type");
            if field.nullable {
                quote::quote! {
                    #value_type(__inner_value) => ::wasm_dbms_api::prelude::Nullable::Value(__inner_value.clone()),
                }
            } else {
                quote::quote! {
                    #value_type(__inner_value) => __inner_value.clone(),
                }
            }
        };
        // `Value::Null` is accepted only by nullable columns
        let null = if field.nullable {
            quote::quote! {
                ::wasm_dbms_api::prelude::Value::Null => ::wasm_dbms_api::prelude::Nullable::Null,
            }
        } else {
            quote::quote! {}
        };

        match_arms.push(quote::quote! {
            #field_name_str => {
                #field_name = Some(match __col_value {
                    #accepted
                    #null
                    _ => #mismatch,
                });
            }
        });
    }

    let mut constructor_fields = vec![];
    for field in &metadata.fields {
        let field_name = &field.name;
        constructor_fields.push(quote::quote! {
            #field_name,
        });
    }

    // locals are prefixed so that they cannot clash with the field bindings declared above
    quote::quote! {
        #[allow(clippy::copy_clone)]
        fn from_values(__wasm_dbms_values: &[(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)], __wasm_dbms_where_clause: Option<::wasm_dbms_api::prelude::Filter>) -> ::wasm_dbms_api::prelude::DbmsResult<Self> {
            #(#field_initializers)*

            for (__wasm_dbms_column, __col_value) in __wasm_dbms_values {
                match __wasm_dbms_column.name {
                    #(#match_arms)*
                    _ => {/* Ignore unknown columns */}
                }
            }

            Ok(Self {
                #(#constructor_fields)*
                where_clause: __wasm_dbms_where_clause,
            })
        }
    }
}

/// Expected to generate for
/// ```rust,ignore
/// pub struct PostUpdateRequest {
///     pub id: Option<Uint32>,
///     pub title: Option<Text>,
///     pub content: Option<Text>,
///     pub user_id: Option<Uint32>,
///     pub where_clause: Option<Filter>,
/// }
/// ```
/// ```rust,ignore
/// fn update_values(&self) -> Vec<(ColumnDef, Value)> {
///     let mut updates = Vec::new();
///
///     if let Some(id) = self.id {
///         updates.push((Self::Schema::columns()[0], Value::Uint32(id)));
///     }
///     if let Some(title) = &self.title {
///         updates.push((Self::Schema::columns()[1], Value::Text(title.clone())));
///     }
///     if let Some(content) = &self.content {
///         updates.push((Self::Schema::columns()[2], Value::Text(content.clone())));
///     }
///     if let Some(user_id) = self.user_id {
///         updates.push((Self::Schema::columns()[3], Value::Uint32(user_id)));
///     }
///
///     updates
/// }
fn impl_update_values(metadata: &TableMetadata) -> TokenStream2 {
    let mut update_values_push = vec![];

    for (index, field) in metadata.fields.iter().enumerate() {
        let field_name = &field.name;
        update_values_push.push(quote::quote! {
            if let Some(value) = &self.#field_name {
                updates.push((Self::Schema::columns()[#index], value.clone().into()));
            }
        });
    }

    quote::quote! {
        fn update_values(&self) -> Vec<(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)> {
            use ::wasm_dbms_api::prelude::TableSchema as _;

            let mut updates = Vec::new();

            #(#update_values_push)*

            updates
        }
    }
}
