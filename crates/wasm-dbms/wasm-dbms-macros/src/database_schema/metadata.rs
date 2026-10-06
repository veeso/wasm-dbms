// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, X-NO-MOD-RS, M-CANONICAL-DOCS

use syn::parse::{Parse, ParseStream};

/// Attribute name for the `#[tables(...)]` annotation.
const ATTRIBUTE_TABLES: &str = "tables";

/// Parsed metadata for a `#[derive(DatabaseSchema)]` invocation.
///
/// Contains the list of table entries extracted from the
/// `#[tables(Type = "name", ...)]` attribute.
pub struct SchemaMetadata {
    /// Registered table entries.
    pub tables: Vec<TableEntry>,
}

/// Parsed metadata for a single table within a `#[tables(...)]` attribute.
pub struct TableEntry {
    /// Type implementing `TableSchema` (e.g. `User` or `Demo<Payload>`).
    pub table: syn::Type,
}

impl Parse for TableEntry {
    /// Parses a `Type = "name"` entry.
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let table: syn::Type = input.parse()?;
        input.parse::<syn::Token![=]>()?;
        input.parse::<syn::LitStr>()?;

        Ok(Self { table })
    }
}

/// Parses `#[tables(User = "users", Post = "posts")]` attributes into
/// [`SchemaMetadata`].
///
/// Table types may carry generic arguments, e.g. `#[tables(Demo<T> = "demo")]`.
pub fn collect_schema_metadata(attrs: &[syn::Attribute]) -> syn::Result<SchemaMetadata> {
    let mut tables = Vec::new();

    for attr in attrs
        .iter()
        .filter(|attr| attr.path().is_ident(ATTRIBUTE_TABLES))
    {
        let entries = attr.parse_args_with(
            syn::punctuated::Punctuated::<TableEntry, syn::Token![,]>::parse_terminated,
        )?;
        tables.extend(entries);
    }

    Ok(SchemaMetadata { tables })
}
