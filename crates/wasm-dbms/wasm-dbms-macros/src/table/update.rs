use proc_macro2::TokenStream as TokenStream2;
use syn::Ident;

use crate::table::metadata::TableMetadata;

pub fn generate_update_request(struct_name: &Ident, metadata: &TableMetadata) -> TokenStream2 {
    let update_request_struct = generate_update_request_struct(metadata);
    let update_record_impl = impl_update_record(struct_name, metadata);

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
fn generate_update_request_struct(metadata: &TableMetadata) -> TokenStream2 {
    let mut fields = vec![];

    for field in &metadata.fields {
        let name = &field.name;
        let value_ty = &field.ty;
        fields.push(quote::quote! {
            pub #name: Option<#value_ty>,
        });
    }

    let update_request_ident = &metadata.update;

    quote::quote! {
        #[derive(Clone, candid::CandidType, serde::Serialize, serde::Deserialize)]
        pub struct #update_request_ident {
            #(#fields)*
            pub where_clause: Option<::wasm_dbms_api::prelude::Filter>,
        }
    }
}

fn impl_update_record(struct_name: &Ident, metadata: &TableMetadata) -> TokenStream2 {
    let update_request_ident = &metadata.update;
    let record_ident = &metadata.record;

    let from_values_impl = impl_from_values(metadata);
    let into_values_impl = impl_update_values(metadata);

    quote::quote! {
        impl ::wasm_dbms_api::prelude::UpdateRecord for #update_request_ident {
            type Record = #record_ident;
            type Schema = #struct_name;

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
/// fn from_values(values: &[(ColumnDef, Value)], where_clause: Option<Filter>) -> Self {
///    let mut id: Option<Uint32> = None;
///    let mut title: Option<Text> = None;
///    let mut content: Option<Text> = None;
///    let mut user_id: Option<Uint32> = None;
///
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
///
///    Self {
///        id,
///        title,
///        content,
///        user_id,
///        where_clause,
///    }
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

        if field.custom_type {
            let custom_ident = field
                .custom_type_ident
                .as_ref()
                .expect("custom_type field must have custom_type_ident");
            if field.nullable {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let ::wasm_dbms_api::prelude::Value::Custom(cv) = value {
                            if let Ok(decoded) = <#custom_ident as ::wasm_dbms_api::prelude::Encode>::decode(
                                std::borrow::Cow::Borrowed(&cv.encoded)
                            ) {
                                #field_name = Some(::wasm_dbms_api::prelude::Nullable::Value(decoded));
                            }
                        } else if let ::wasm_dbms_api::prelude::Value::Null = value {
                            #field_name = Some(::wasm_dbms_api::prelude::Nullable::Null);
                        }
                    }
                })
            } else {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let ::wasm_dbms_api::prelude::Value::Custom(cv) = value {
                            if let Ok(decoded) = <#custom_ident as ::wasm_dbms_api::prelude::Encode>::decode(
                                std::borrow::Cow::Borrowed(&cv.encoded)
                            ) {
                                #field_name = Some(decoded);
                            }
                        }
                    }
                })
            }
        } else {
            let value_type = field
                .value_type
                .as_ref()
                .expect("built-in field must have value_type");

            if field.nullable {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let #value_type(v) = value {
                            #field_name = Some(::wasm_dbms_api::prelude::Nullable::Value(v.clone()));
                        } else if let ::wasm_dbms_api::prelude::Value::Null = value {
                            #field_name = Some(::wasm_dbms_api::prelude::Nullable::Null);
                        }
                    }
                })
            } else {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let #value_type(v) = value {
                            #field_name = Some(v.clone());
                        }
                    }
                })
            }
        }
    }

    let mut constructor_fields = vec![];
    for field in &metadata.fields {
        let field_name = &field.name;
        constructor_fields.push(quote::quote! {
            #field_name,
        });
    }

    quote::quote! {
        #[allow(clippy::copy_clone)]
        fn from_values(values: &[(::wasm_dbms_api::prelude::ColumnDef, ::wasm_dbms_api::prelude::Value)], where_clause: Option<::wasm_dbms_api::prelude::Filter>) -> Self {
            #(#field_initializers)*

            for (column, value) in values {
                match column.name {
                    #(#match_arms)*
                    _ => {/* Ignore unknown columns */}
                }
            }

            Self {
                #(#constructor_fields)*
                where_clause,
            }
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
