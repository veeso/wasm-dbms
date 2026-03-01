// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, X-NO-MOD-RS, M-CANONICAL-DOCS

use syn::Ident;

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
    /// Struct identifier implementing `TableSchema` (e.g. `User`).
    pub table: Ident,
    /// Generated insert request type identifier (e.g. `UserInsertRequest`).
    pub insert: Ident,
    /// Generated update request type identifier (e.g. `UserUpdateRequest`).
    pub update: Ident,
}

/// Parses `#[tables(User = "users", Post = "posts")]` attributes into
/// [`SchemaMetadata`].
pub fn collect_schema_metadata(attrs: &[syn::Attribute]) -> syn::Result<SchemaMetadata> {
    let mut tables = Vec::new();
    let mut names = vec![];

    for attr in attrs {
        if attr.path().is_ident(ATTRIBUTE_TABLES) {
            attr.parse_nested_meta(|meta| {
                let ident = meta
                    .path
                    .get_ident()
                    .cloned()
                    .ok_or_else(|| meta.error("expected identifier"))?;
                let value: syn::LitStr = meta.value()?.parse()?;
                let value = value.value();

                names.push((ident, value));

                Ok(())
            })?;
        }
    }

    for (ident, _name) in names {
        tables.push(collect_table_entry(ident)?);
    }

    Ok(SchemaMetadata { tables })
}

/// Derives associated type identifiers for a single table entry.
fn collect_table_entry(table: Ident) -> syn::Result<TableEntry> {
    let insert_ident = Ident::new(&format!("{table}InsertRequest"), table.span());
    let update_ident = Ident::new(&format!("{table}UpdateRequest"), table.span());

    Ok(TableEntry {
        table: table.clone(),
        insert: insert_ident,
        update: update_ident,
    })
}
