use std::collections::HashMap;

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::ToTokens as _;
use syn::{DataStruct, Ident};

const MIN_ALIGNMENT: u16 = 8;

const ATTRIBUTE_ALIGNMENT: &str = "alignment";
const ATTRIBUTE_TABLE: &str = "table";
const ATTRIBUTE_PRIMARY_KEY: &str = "primary_key";
const ATTRIBUTE_FOREIGN_KEY: &str = "foreign_key";
const ATTRIBUTE_FOREIGN_KEY_ENTITY: &str = "entity";
const ATTRIBUTE_FOREIGN_KEY_TABLE: &str = "table";
const ATTRIBUTE_FOREIGN_KEY_COLUMN: &str = "column";

/// Representation of a foreign key in a table
pub struct ForeignKey {
    /// Entity referenced (e.g. `User`)
    pub entity: Ident,
    /// Name of the field in the current table that is a foreign key
    pub field: Ident,
    /// The record type to return (e.g. `UserRecord`)
    pub record_type: Ident,
    /// Name of the referenced table (e.g. `users`)
    pub referenced_table: Ident,
    /// Name of the referenced field in the referenced table
    pub referenced_field: Ident,
}

/// Field metadata
pub struct Field {
    /// Name of the field
    pub name: Ident,
    /// Type of the field
    pub ty: syn::Path,
    /// Data type kind of the field; e.g. `DataTypeKind::Int32` or `DataTypeKind::Custom("tag")`
    pub data_type_kind: syn::Expr,
    /// Whether the field is a foreign key
    pub is_fk: bool,
    /// Whether the field is nullable
    pub nullable: bool,
    /// Whether the field is a primary key
    pub primary_key: bool,
    /// Whether the field uses `#[custom_type]`
    pub custom_type: bool,
    /// Sanitize struct to use for this field
    pub sanitize: Option<Sanitizer>,
    /// Validate struct to use for this field
    pub validate: Option<Validator>,
    /// Value type of the field; e.g. `Value::Int32`. `None` for custom types.
    pub value_type: Option<syn::Path>,
}

/// Validator metadata
#[derive(Clone)]
pub struct Validator {
    pub path: syn::Path,
    pub args: Vec<syn::Expr>,
}

/// Map of field identifiers to their validators
type Validates = HashMap<Ident, Validator>;

/// Sanitizer metadata
#[derive(Clone)]
pub enum Sanitizer {
    /// A sanitizer for a unit struct
    Unit { name: syn::Path },
    /// A sanitizer represented by a tuple struct with arguments
    Tuple {
        name: syn::Path,
        args: Vec<syn::Expr>,
    },
    /// A sanitizer represented by a struct with named arguments
    NamedArgs {
        name: syn::Path,
        args: HashMap<Ident, syn::Expr>,
    },
}

/// Map of field identifiers to their sanitizers
type Sanitizers = HashMap<Ident, Sanitizer>;

/// Metadata about the table extracted from the struct and its attributes
pub struct TableMetadata {
    /// Name of the table
    pub name: Ident,
    /// Name of the primary key field
    pub primary_key: Ident,
    /// List of foreign keys
    pub foreign_keys: Vec<ForeignKey>,
    /// Name of the record type
    pub record: Ident,
    /// Name of the insert type
    pub insert: Ident,
    /// Name of the update type
    pub update: Ident,
    /// Name of the foreign fetcher type; set only if there are foreign keys
    pub foreign_fetcher: Option<Ident>,
    /// Fields; the order is preserved
    pub fields: Vec<Field>,
    /// Memory alignment if provided
    pub alignment: Option<u16>,
}

impl TableMetadata {
    /// Get the identifier for the foreign fetcher, or default to `NoForeignFetcher` if none is set
    pub fn foreign_fetcher_ident(&self) -> TokenStream2 {
        match self.foreign_fetcher.as_ref() {
            Some(ident) => quote::quote! { #ident },
            None => quote::quote! { ::ic_dbms_api::prelude::NoForeignFetcher },
        }
    }
}

/// Collect metadata about the table from the struct and its attributes
///
/// # Panics
///
/// - If the struct does not have a `table` attribute
/// - If the struct does not have a field marked as primary key or if multiple primary keys are found
pub fn collect_table_metadata(
    struct_name: &Ident,
    data: &DataStruct,
    attrs: &[syn::Attribute],
) -> syn::Result<TableMetadata> {
    let alignment = get_alignment(attrs)?;
    let table_name = get_table_name(attrs)?;
    let primary_key = get_primary_key_field(data)?;
    let foreign_keys = collect_foreign_keys(data)?;
    let validates = collect_validates(data)?;
    let sanitizes = collect_sanitizes(data)?;
    let record_ident = Ident::new(&format!("{struct_name}Record"), struct_name.span());
    let insert_ident = Ident::new(&format!("{struct_name}InsertRequest"), struct_name.span());
    let update_ident = Ident::new(&format!("{struct_name}UpdateRequest"), struct_name.span());
    let foreign_fetcher_ident = if !foreign_keys.is_empty() {
        Some(Ident::new(
            &format!("{struct_name}ForeignFetcher"),
            struct_name.span(),
        ))
    } else {
        None
    };
    let fields = get_fields(data, &primary_key, &foreign_keys, &sanitizes, &validates)?;

    Ok(TableMetadata {
        name: table_name,
        primary_key,
        foreign_keys,
        record: record_ident,
        insert: insert_ident,
        update: update_ident,
        foreign_fetcher: foreign_fetcher_ident,
        fields,
        alignment,
    })
}

/// Extract the alignment from the `alignment` attribute
fn get_alignment(attrs: &[syn::Attribute]) -> syn::Result<Option<u16>> {
    for attr in attrs {
        if attr.path().is_ident(ATTRIBUTE_ALIGNMENT) {
            // syntax is #[alignment = 16]
            let expr = &attr
                .meta
                .require_name_value()
                .expect("invalid syntax for `table` attribute")
                .value;

            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(lit),
                ..
            }) = expr
            {
                let alignment: u16 = lit.base10_parse().map_err(|_| {
                    syn::Error::new_spanned(lit, "alignment must be a valid unsigned integer")
                })?;
                if alignment < MIN_ALIGNMENT {
                    return Err(syn::Error::new_spanned(
                        lit,
                        format!("alignment must be at least {MIN_ALIGNMENT}"),
                    ));
                }

                return Ok(Some(alignment));
            } else {
                return Err(syn::Error::new_spanned(expr, "expected number literal"));
            }
        }
    }

    Ok(None)
}

/// Extract the table name from the `table` attribute
fn get_table_name(attrs: &[syn::Attribute]) -> syn::Result<Ident> {
    for attr in attrs {
        if attr.path().is_ident(ATTRIBUTE_TABLE) {
            // syntax is #[table = "table_name"]
            let expr = &attr
                .meta
                .require_name_value()
                .expect("invalid syntax for `table` attribute")
                .value;

            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit),
                ..
            }) = expr
            {
                let table_name = lit.value();
                return Ok(Ident::new(&table_name, lit.span()));
            } else {
                return Err(syn::Error::new_spanned(expr, "expected string literal"));
            }
        }
    }

    Err(syn::Error::new(
        proc_macro2::Span::call_site(),
        "missing `table` attribute",
    ))
}

/// Find the primary key field in the struct
fn get_primary_key_field(data: &DataStruct) -> syn::Result<Ident> {
    let mut primary_key = None;

    for field in &data.fields {
        for attr in &field.attrs {
            if attr.path().is_ident(ATTRIBUTE_PRIMARY_KEY) {
                if primary_key.is_some() {
                    return Err(syn::Error::new_spanned(
                        field,
                        "multiple primary keys found",
                    ));
                }
                primary_key = Some(field.ident.clone().ok_or(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "primary_key should be a named field",
                ))?);
            }
        }
    }

    if let Some(pk) = primary_key {
        Ok(pk)
    } else {
        Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "no primary key found",
        ))
    }
}

/// Collect foreign keys from the struct fields
fn collect_foreign_keys(data: &DataStruct) -> syn::Result<Vec<ForeignKey>> {
    let mut foreign_keys = Vec::new();
    for field in &data.fields {
        for attr in &field.attrs {
            if attr.path().is_ident(ATTRIBUTE_FOREIGN_KEY) {
                let mut referenced_entity = None;
                let mut referenced_table = None;
                let mut referenced_field = None;

                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident(ATTRIBUTE_FOREIGN_KEY_ENTITY) {
                        let lit: syn::LitStr = meta.value()?.parse()?;
                        referenced_entity = Some(Ident::new(&lit.value(), lit.span()));
                        return Ok(());
                    }
                    if meta.path.is_ident(ATTRIBUTE_FOREIGN_KEY_TABLE) {
                        let lit: syn::LitStr = meta.value()?.parse()?;
                        referenced_table = Some(Ident::new(&lit.value(), lit.span()));
                        return Ok(());
                    }
                    if meta.path.is_ident(ATTRIBUTE_FOREIGN_KEY_COLUMN) {
                        let lit: syn::LitStr = meta.value()?.parse()?;
                        referenced_field = Some(Ident::new(&lit.value(), lit.span()));
                        return Ok(());
                    }
                    Ok(())
                })?;

                let entity = referenced_entity.ok_or(syn::Error::new_spanned(
                    attr,
                    "missing `entity` in foreign_key attribute",
                ))?;
                let record = Ident::new(&format!("{}Record", entity), entity.span());

                let fk = ForeignKey {
                    entity,
                    field: field.ident.clone().ok_or(syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "foreign_key should be a named field",
                    ))?,
                    referenced_table: referenced_table.ok_or(syn::Error::new_spanned(
                        attr,
                        "missing `table` in foreign_key attribute",
                    ))?,
                    referenced_field: referenced_field.ok_or(syn::Error::new_spanned(
                        attr,
                        "missing `column` in foreign_key attribute",
                    ))?,
                    record_type: record,
                };

                foreign_keys.push(fk);
            }
        }
    }

    Ok(foreign_keys)
}

fn collect_validates(data: &DataStruct) -> syn::Result<Validates> {
    let mut validates = HashMap::new();

    for field in &data.fields {
        for attr in &field.attrs {
            if attr.path().is_ident("validate") {
                let validator = match attr.parse_args::<syn::Expr>()? {
                    syn::Expr::Path(expr) => Validator {
                        path: expr.path,
                        args: Vec::new(),
                    },
                    syn::Expr::Call(call) => {
                        let path = match *call.func {
                            syn::Expr::Path(p) => p.path,
                            other => {
                                return Err(syn::Error::new_spanned(
                                    other,
                                    "validator must be a path or a call, e.g. Validator or Validator(42)",
                                ));
                            }
                        };

                        Validator {
                            path,
                            args: call.args.into_iter().collect(),
                        }
                    }
                    other => {
                        return Err(syn::Error::new_spanned(other, "invalid validator syntax"));
                    }
                };

                let ident = field.ident.clone().ok_or_else(|| {
                    syn::Error::new_spanned(field, "validate can only be used on named fields")
                })?;

                validates.insert(ident, validator);
            }
        }
    }

    Ok(validates)
}

fn collect_sanitizes(data: &DataStruct) -> syn::Result<Sanitizers> {
    let mut sanitizers = HashMap::new();

    for field in &data.fields {
        for attr in &field.attrs {
            if attr.path().is_ident("sanitizer") {
                let sanitizer = if let Some(sanitizer) = parse_sanitizer_meta(attr)? {
                    sanitizer
                } else {
                    parse_sanitizer_expr(attr)?
                };

                let ident = field.ident.clone().ok_or_else(|| {
                    syn::Error::new_spanned(field, "validate can only be used on named fields")
                })?;

                sanitizers.insert(ident, sanitizer);
            }
        }
    }

    Ok(sanitizers)
}

fn parse_sanitizer_meta(attr: &syn::Attribute) -> syn::Result<Option<Sanitizer>> {
    let syn::Meta::List(meta_list) = &attr.meta else {
        return Ok(None);
    };

    let mut path: Option<syn::Path> = None;
    let mut args = HashMap::new();

    meta_list.parse_nested_meta(|meta| {
        // FIRST: sanitizer path (must NOT be name = value)
        if path.is_none() {
            if meta.input.peek(syn::Token![=]) {
                return Err(syn::Error::new_spanned(
                    meta.path,
                    "first sanitizer argument must be a path",
                ));
            }

            path = Some(meta.path.clone());
            return Ok(());
        }

        // named args: min = 0
        let ident = meta
            .path
            .get_ident()
            .ok_or_else(|| syn::Error::new_spanned(&meta.path, "expected identifier"))?;

        let value = meta.value()?.parse::<syn::Expr>()?;
        args.insert(ident.clone(), value);

        Ok(())
    })?;

    if let Some(path) = path
        && !args.is_empty()
    {
        Ok(Some(Sanitizer::NamedArgs { name: path, args }))
    } else {
        Ok(None)
    }
}

fn parse_sanitizer_expr(attr: &syn::Attribute) -> syn::Result<Sanitizer> {
    match attr.parse_args::<syn::Expr>()? {
        syn::Expr::Path(expr) => Ok(Sanitizer::Unit { name: expr.path }),

        syn::Expr::Call(call) => {
            let path = match *call.func {
                syn::Expr::Path(p) => p.path,
                other => {
                    return Err(syn::Error::new_spanned(
                        other,
                        "sanitizer must be a path or a call",
                    ));
                }
            };

            Ok(Sanitizer::Tuple {
                name: path,
                args: call.args.into_iter().collect(),
            })
        }

        other => Err(syn::Error::new_spanned(other, "invalid sanitizer syntax")),
    }
}

fn get_fields(
    data: &DataStruct,
    primary_key: &Ident,
    foreign_keys: &[ForeignKey],
    sanitizes: &Sanitizers,
    validates: &Validates,
) -> syn::Result<Vec<Field>> {
    let mut fields = vec![];

    for field in &data.fields {
        let name = field
            .ident
            .as_ref()
            .cloned()
            .ok_or(syn::Error::new_spanned(field, "All fields must be named"))?;
        let field_type = &field.ty;
        let field_type_name = field_type.to_token_stream();
        let primary_key = &name == primary_key;

        let is_fk = foreign_keys.iter().any(|fk| fk.field == name);

        let sanitize = sanitizes.get(&name).cloned();
        let validate = validates.get(&name).cloned();

        let nullable = nullable(field);
        // if is nullable the data type is the inner type
        // Step 1: estrai il nome (con gestione Nullable)
        let field_type_name_str = if nullable {
            let type_str = field_type_name.to_string();
            let inner = type_str
                .strip_prefix("Nullable <")
                .and_then(|s| s.strip_suffix('>'))
                .ok_or_else(|| syn::Error::new_spanned(field, "invalid Nullable type syntax"))?
                .trim();
            inner.to_string()
        } else {
            field_type_name.to_string()
        };

        // get full type for `ty`
        let ty: syn::Path = syn::parse_quote! {
            #field_type
        };

        // Step 2: detect custom_type attribute
        let custom_type = is_custom_type(field);

        // Step 3: build data_type_kind and value_type
        let (data_type_kind, value_type): (syn::Expr, Option<syn::Path>) = if custom_type {
            let dtk: syn::Expr = syn::parse_quote! {
                ::ic_dbms_api::prelude::DataTypeKind::Custom(
                    <#field_type_name as ::ic_dbms_api::prelude::CustomDataType>::TYPE_TAG
                )
            };
            (dtk, None)
        } else {
            let field_type_ident = syn::Ident::new(&field_type_name_str, Span::call_site());
            let dtk: syn::Path = syn::parse_quote! {
                ::ic_dbms_api::prelude::DataTypeKind::#field_type_ident
            };
            let vt: syn::Path = syn::parse_quote! {
                ::ic_dbms_api::prelude::Value::#field_type_ident
            };
            (
                syn::Expr::Path(syn::ExprPath {
                    attrs: vec![],
                    qself: None,
                    path: dtk,
                }),
                Some(vt),
            )
        };

        fields.push(Field {
            name,
            is_fk,
            ty,
            data_type_kind,
            nullable,
            primary_key,
            custom_type,
            sanitize,
            validate,
            value_type,
        });
    }

    Ok(fields)
}

/// If the type of field is `Nullable<T>`, returns `true`, else `false`.
fn nullable(field: &syn::Field) -> bool {
    let field_type = &field.ty;
    let field_type_name = field_type.to_token_stream();
    field_type_name.to_string().starts_with("Nullable <")
}

/// Returns `true` if the field has a `#[custom_type]` attribute.
fn is_custom_type(field: &syn::Field) -> bool {
    field
        .attrs
        .iter()
        .any(|attr| attr.path().is_ident("custom_type"))
}
