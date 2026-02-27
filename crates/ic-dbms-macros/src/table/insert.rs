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
        fields.push(quote::quote! {
            pub #name: #value_ty,
        });
    }

    let insert_request_ident = &metadata.insert;

    quote::quote! {
        #[derive(Clone, candid::CandidType, serde::Serialize, serde::Deserialize)]
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
        impl ::ic_dbms_api::prelude::InsertRecord for #insert_request_ident {
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
///fn from_values(values: &[(ColumnDef, Value)]) -> ic_dbms_api::prelude::IcDbmsResult<Self> {
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
///        id: id.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
///            "id".to_string(),
///        )))?,
///        title: title.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
///            "title".to_string(),
///        )))?,
///        content: content.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
///            "content".to_string(),
///        )))?,
///        user_id: user_id.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
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

        declare_lets.push(quote::quote! {
            let mut #name: Option<#ty> = None;
        });
    }

    let mut match_arms = vec![];
    for field in &metadata.fields {
        let field_name = &field.name;
        let field_name_str = field.name.to_string();

        if field.custom_type {
            let custom_ident = field.custom_type_ident.as_ref().expect("custom_type field must have custom_type_ident");
            if field.nullable {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let ::ic_dbms_api::prelude::Value::Custom(cv) = value {
                            #field_name = Some(::ic_dbms_api::prelude::Nullable::Value(
                                <#custom_ident as ::ic_dbms_api::prelude::Encode>::decode(
                                    std::borrow::Cow::Borrowed(&cv.encoded)
                                ).expect("failed to decode custom type")
                            ));
                        } else if let ::ic_dbms_api::prelude::Value::Null = value {
                            #field_name = Some(::ic_dbms_api::prelude::Nullable::Null);
                        }
                    }
                });
            } else {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let ::ic_dbms_api::prelude::Value::Custom(cv) = value {
                            #field_name = Some(
                                <#custom_ident as ::ic_dbms_api::prelude::Encode>::decode(
                                    std::borrow::Cow::Borrowed(&cv.encoded)
                                ).expect("failed to decode custom type")
                            );
                        }
                    }
                });
            }
        } else {
            let value_type = field.value_type.as_ref().expect("built-in field must have value_type");

            if field.nullable {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let #value_type(value) = value {
                            #field_name = Some(::ic_dbms_api::prelude::Nullable::Value(value.clone()));
                        } else if let ::ic_dbms_api::prelude::Value::Null = value {
                            #field_name = Some(::ic_dbms_api::prelude::Nullable::Null);
                        }
                    }
                });
            } else {
                match_arms.push(quote::quote! {
                    #field_name_str => {
                        if let #value_type(value) = value {
                            #field_name = Some(value.clone());
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

        if field.nullable {
            struct_fields.push(quote::quote! {
                #name: #name.unwrap_or(::ic_dbms_api::prelude::Nullable::Null),
            })
        } else {
            struct_fields.push(quote::quote! {
                #name: #name.ok_or(::ic_dbms_api::prelude::IcDbmsError::Query(::ic_dbms_api::prelude::QueryError::MissingNonNullableField(
                    #name_str.to_string(),
                )))?,
            })
        }
    }

    quote::quote! {
        #[allow(clippy::copy_clone)]
        fn from_values(values: &[(::ic_dbms_api::prelude::ColumnDef, ::ic_dbms_api::prelude::Value)]) -> ::ic_dbms_api::prelude::IcDbmsResult<Self> {
            #(#declare_lets)*

            for (column, value) in values {
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
    let mut fields = vec![];
    for (index, field) in metadata.fields.iter().enumerate() {
        let field = &field.name;
        fields.push(quote::quote! {
            (Self::Schema::columns()[#index], self.#field.into())
        })
    }

    quote::quote! {
        fn into_values(self) -> Vec<(::ic_dbms_api::prelude::ColumnDef, ::ic_dbms_api::prelude::Value)> {
            use ::ic_dbms_api::prelude::TableSchema as _;

            vec![
                #(#fields),*
            ]
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
        fields.push(quote::quote! {
            #name: self.#name,
        });
    }

    quote::quote! {
        fn into_record(self) -> Self::Schema {
            Self::Schema {
                #(#fields)*
            }
        }
    }
}
