use std::collections::HashMap;

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::ToTokens as _;
use syn::{DataStruct, Ident};

const MIN_ALIGNMENT: u16 = 8;

const ATTRIBUTE_ALIGNMENT: &str = "alignment";
const ATTRIBUTE_TABLE: &str = "table";
const ATTRIBUTE_INDEX: &str = "index";
const ATTRIBUTE_UNIQUE: &str = "unique";
const ATTRIBUTE_PRIMARY_KEY: &str = "primary_key";
const ATTRIBUTE_FOREIGN_KEY: &str = "foreign_key";
const ATTRIBUTE_FOREIGN_KEY_ENTITY: &str = "entity";
const ATTRIBUTE_FOREIGN_KEY_TABLE: &str = "table";
const ATTRIBUTE_FOREIGN_KEY_COLUMN: &str = "column";
const ATTRIBUTE_DEFAULT: &str = "default";
const ATTRIBUTE_RENAMED_FROM: &str = "renamed_from";
const ATTRIBUTE_MIGRATE: &str = "migrate";

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
    /// Whether the field is auto-incrementing (i.e. `#[autoincrement]`); only valid for integer primary keys
    /// Conflicts with `nullable`.
    /// Applicable only for numeric types.
    pub auto_increment: bool,
    /// Whether the field is a primary key
    pub primary_key: bool,
    /// Whether the field is unique
    pub unique: bool,
    /// Whether the field uses `#[custom_type]`
    pub custom_type: bool,
    /// For custom types: the inner type ident (with Nullable stripped).
    /// Used in codegen for CustomDataType::TYPE_TAG and Encode::decode lookups.
    pub custom_type_ident: Option<syn::Ident>,
    /// Sanitize struct to use for this field
    pub sanitize: Option<Sanitizer>,
    /// Validate struct to use for this field
    pub validate: Option<Validator>,
    /// Value type of the field; e.g. `Value::Int32`. `None` for custom types.
    pub value_type: Option<syn::Path>,
    /// Default value literal, if `#[default = ...]` is set on the field.
    ///
    /// The expression is taken verbatim and wrapped in a closure at codegen
    /// time so the resulting [`ColumnDef::default`] is a `fn() -> Value`.
    pub default: Option<syn::Expr>,
    /// Previous names this field was known by, declared via
    /// `#[renamed_from("old1", "old2", ...)]`.
    pub renamed_from: Vec<String>,
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

/// Represents a resolved index definition, built from `#[index]` field attributes.
///
/// - A bare `#[index]` creates a single-column index.
/// - `#[index(group = "name")]` groups fields sharing the same group into a composite index.
/// - The primary key always produces an implicit index.
pub struct Index {
    /// Column names that make up this index, in field declaration order.
    pub columns: Vec<Ident>,
}

/// Raw per-field index annotation: either standalone or grouped.
enum FieldIndex {
    /// Bare `#[index]` -- standalone single-column index.
    Standalone,
    /// `#[index(group = "name")]` -- part of a composite index.
    Grouped(String),
}

/// Metadata about the table extracted from the struct and its attributes
pub struct TableMetadata {
    /// Name of the table
    pub name: Ident,
    /// Name of the primary key field
    pub primary_key: Ident,
    /// List of foreign keys
    pub foreign_keys: Vec<ForeignKey>,
    /// List of indexes
    pub indexes: Vec<Index>,
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
    /// Whether to add `candid::CandidType` and `serde::{Serialize, Deserialize}` derives
    /// to generated Record, Insert, and Update types
    pub candid: bool,
    /// Set when the struct carries `#[migrate]`, suppressing the default
    /// `impl Migrate for T {}` emission so the user can provide their own.
    pub user_migrate_impl: bool,
}

impl TableMetadata {
    /// Get the identifier for the foreign fetcher, or default to `NoForeignFetcher` if none is set
    pub fn foreign_fetcher_ident(&self) -> TokenStream2 {
        match self.foreign_fetcher.as_ref() {
            Some(ident) => quote::quote! { #ident },
            None => quote::quote! { ::wasm_dbms_api::prelude::NoForeignFetcher },
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
    let unique_fields = get_unique_fields(data);
    let indexes = collect_indexes(data, &primary_key, &unique_fields)?;
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
    let candid = attrs.iter().any(|a| a.path().is_ident("candid"));
    let user_migrate_impl = attrs.iter().any(|a| a.path().is_ident(ATTRIBUTE_MIGRATE));

    Ok(TableMetadata {
        name: table_name,
        primary_key,
        foreign_keys,
        indexes,
        record: record_ident,
        insert: insert_ident,
        update: update_ident,
        foreign_fetcher: foreign_fetcher_ident,
        fields,
        alignment,
        candid,
        user_migrate_impl,
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

/// Collect indexes from field-level `#[index]` / `#[index(group = "...")]` attributes.
///
/// The primary key always produces an implicit single-column index (listed first).
/// Bare `#[index]` fields each produce a single-column index.
/// Fields sharing the same `group` name are merged into one composite index,
/// with columns ordered by field declaration order.
fn collect_indexes(
    data: &DataStruct,
    primary_key: &Ident,
    unique: &[Ident],
) -> syn::Result<Vec<Index>> {
    // PK is always an index.
    let mut indexes = vec![Index {
        columns: vec![primary_key.clone()],
    }];
    // Unique fields also always have an index, but we skip them if they are the primary key since it's redundant.
    for unique in unique {
        if unique != primary_key {
            indexes.push(Index {
                columns: vec![(*unique).clone()],
            });
        }
    }

    // Collect per-field annotations: (field_name, FieldIndex).
    let mut grouped: HashMap<String, Vec<Ident>> = HashMap::new();

    for field in &data.fields {
        for attr in &field.attrs {
            if attr.path().is_ident(ATTRIBUTE_INDEX) {
                let field_name = field.ident.clone().ok_or_else(|| {
                    syn::Error::new_spanned(field, "`#[index]` can only be used on named fields")
                })?;

                // Skip redundant `#[index]` on the primary key — it already has an implicit index.
                // skip also redundant `#[index]` on unique fields since they also have implicit indexes.
                if &field_name == primary_key && !unique.contains(&field_name) {
                    continue;
                }

                let field_index = parse_index_attr(attr)?;

                match field_index {
                    FieldIndex::Standalone => {
                        indexes.push(Index {
                            columns: vec![field_name],
                        });
                    }
                    FieldIndex::Grouped(group) => {
                        grouped.entry(group).or_default().push(field_name);
                    }
                }
            }
        }
    }

    // Append grouped composite indexes (sorted by group name for determinism).
    let mut group_names: Vec<_> = grouped.keys().cloned().collect();
    group_names.sort();
    for name in group_names {
        let columns = grouped.remove(&name).expect("key must exist");
        indexes.push(Index { columns });
    }

    Ok(indexes)
}

/// Parse a single `#[index]` or `#[index(group = "...")]` attribute.
fn parse_index_attr(attr: &syn::Attribute) -> syn::Result<FieldIndex> {
    // Bare `#[index]` -- no parentheses at all.
    if matches!(&attr.meta, syn::Meta::Path(_)) {
        return Ok(FieldIndex::Standalone);
    }

    let mut group: Option<String> = None;

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("group") {
            let lit: syn::LitStr = meta.value()?.parse()?;
            group = Some(lit.value());
            return Ok(());
        }
        Err(syn::Error::new_spanned(
            &meta.path,
            "unknown index attribute; expected `group`",
        ))
    })?;

    match group {
        Some(g) => Ok(FieldIndex::Grouped(g)),
        None => Err(syn::Error::new_spanned(
            attr,
            "`#[index(...)]` requires `group = \"name\"`",
        )),
    }
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

fn get_unique_fields(data: &DataStruct) -> Vec<Ident> {
    let mut unique_fields = Vec::new();

    for field in &data.fields {
        for attr in &field.attrs {
            if attr.path().is_ident(ATTRIBUTE_UNIQUE) {
                let field_name = field
                    .ident
                    .clone()
                    .expect("unique can only be used on named fields");
                unique_fields.push(field_name);
            }
        }
    }

    unique_fields
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

        // Step 2: detect field attributes
        let custom_type = is_custom_type(field);
        let unique = unique(field);
        let autoincrement = autoincrement(field)?;

        // Validate: #[custom_type] and #[foreign_key] cannot be combined
        if custom_type && is_fk {
            return Err(syn::Error::new_spanned(
                field,
                "`#[custom_type]` and `#[foreign_key]` cannot be used on the same field",
            ));
        }
        // Validate: #[autoincrement] cannot be combined with #[nullable], since autoincrement fields must have a value and cannot be null
        if autoincrement && nullable {
            return Err(syn::Error::new_spanned(
                field,
                "`#[autoincrement]` fields cannot be nullable",
            ));
        }

        // Step 3: build data_type_kind and value_type
        let field_type_ident = syn::Ident::new(&field_type_name_str, Span::call_site());
        let (data_type_kind, value_type, custom_type_ident): (
            syn::Expr,
            Option<syn::Path>,
            Option<syn::Ident>,
        ) = if custom_type {
            let custom_ident = field_type_ident.clone();
            let dtk: syn::Expr = syn::parse_quote! {
                ::wasm_dbms_api::prelude::DataTypeKind::Custom {
                    tag: <#custom_ident as ::wasm_dbms_api::prelude::CustomDataType>::TYPE_TAG,
                    wire_size: ::wasm_dbms_api::prelude::WireSize::from_data_size(
                        <#custom_ident as ::wasm_dbms_api::prelude::Encode>::SIZE,
                    ),
                }
            };
            (dtk, None, Some(custom_ident))
        } else {
            let dtk: syn::Path = syn::parse_quote! {
                ::wasm_dbms_api::prelude::DataTypeKind::#field_type_ident
            };
            let vt: syn::Path = syn::parse_quote! {
                ::wasm_dbms_api::prelude::Value::#field_type_ident
            };
            (
                syn::Expr::Path(syn::ExprPath {
                    attrs: vec![],
                    qself: None,
                    path: dtk,
                }),
                Some(vt),
                None,
            )
        };

        let default = parse_default(field)?;
        let renamed_from = parse_renamed_from(field)?;

        fields.push(Field {
            name,
            is_fk,
            ty,
            data_type_kind,
            nullable,
            auto_increment: autoincrement,
            unique,
            primary_key,
            custom_type,
            custom_type_ident,
            sanitize,
            validate,
            value_type,
            default,
            renamed_from,
        });
    }

    Ok(fields)
}

/// Parses the optional `#[default = <expr>]` attribute on a field.
///
/// The expression is taken verbatim and used at codegen time to build a
/// `fn() -> Value` constructor; type compatibility against the column data
/// type is enforced by `rustc` when the generated code is compiled, since
/// `Value::from(<expr>)` is type-checked against the column's `Value`
/// variant.
fn parse_default(field: &syn::Field) -> syn::Result<Option<syn::Expr>> {
    let mut found: Option<syn::Expr> = None;

    for attr in &field.attrs {
        if !attr.path().is_ident(ATTRIBUTE_DEFAULT) {
            continue;
        }
        let name_value = attr.meta.require_name_value().map_err(|_| {
            syn::Error::new_spanned(
                attr,
                "expected `#[default = <expr>]` (e.g. `#[default = 0]`)",
            )
        })?;
        if found.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "duplicate `#[default]` attribute",
            ));
        }
        found = Some(name_value.value.clone());
    }

    Ok(found)
}

/// Parses the optional `#[renamed_from("a", "b", ...)]` attribute on a field.
///
/// Each entry must be a string literal; non-string entries produce a compile
/// error so the migration planner does not pick up garbage column names.
fn parse_renamed_from(field: &syn::Field) -> syn::Result<Vec<String>> {
    let mut names: Vec<String> = Vec::new();
    let mut seen = false;

    for attr in &field.attrs {
        if !attr.path().is_ident(ATTRIBUTE_RENAMED_FROM) {
            continue;
        }
        if seen {
            return Err(syn::Error::new_spanned(
                attr,
                "duplicate `#[renamed_from]` attribute",
            ));
        }
        seen = true;

        let list = attr.meta.require_list().map_err(|_| {
            syn::Error::new_spanned(attr, "expected `#[renamed_from(\"old1\", \"old2\", ...)]`")
        })?;
        let punctuated = list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::LitStr, syn::Token![,]>::parse_terminated,
            )
            .map_err(|_| {
                syn::Error::new_spanned(
                    attr,
                    "`#[renamed_from(...)]` entries must be string literals",
                )
            })?;
        for lit in punctuated {
            names.push(lit.value());
        }

        if names.is_empty() {
            return Err(syn::Error::new_spanned(
                attr,
                "`#[renamed_from(...)]` requires at least one previous name",
            ));
        }
    }

    Ok(names)
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

/// Returns `true` if the field has a `#[unique]` attribute.
fn unique(field: &syn::Field) -> bool {
    field
        .attrs
        .iter()
        .any(|attr| attr.path().is_ident("unique"))
}

/// Check whethers the field has a `#[autoincrement]` attribute; only valid for integer primary keys
fn autoincrement(field: &syn::Field) -> syn::Result<bool> {
    let autoincrement = field
        .attrs
        .iter()
        .any(|attr| attr.path().is_ident("autoincrement"));

    if !autoincrement {
        return Ok(false);
    }

    // Validate that autoincrement is only used on integer primary keys
    let field_type = &field.ty;
    let field_type_name = field_type.to_token_stream().to_string();
    let is_integer = matches!(
        field_type_name.as_str(),
        "Int8" | "Int16" | "Int32" | "Int64" | "Uint8" | "Uint16" | "Uint32" | "Uint64"
    );
    if !is_integer {
        return Err(syn::Error::new_spanned(
            field,
            "`#[autoincrement]` can only be used on integer primary keys",
        ));
    }

    Ok(true)
}
