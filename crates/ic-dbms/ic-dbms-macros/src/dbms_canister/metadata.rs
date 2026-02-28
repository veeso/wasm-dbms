use syn::Ident;

const ATTRIBUTE_TABLES: &str = "tables";

pub struct CanisterMetadata {
    pub tables: Vec<TableMetadata>,
}

pub struct TableMetadata {
    pub name: String,
    pub table: Ident,
    pub record: Ident,
    pub insert: Ident,
    pub update: Ident,
}

/// Collects canister metadata from the given attributes.
pub fn collect_canister_metadata(attrs: &[syn::Attribute]) -> syn::Result<CanisterMetadata> {
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
            })
            .expect("invalid syntax in #[tables]");
        }
    }

    for (ident, name) in names {
        tables.push(collect_table_metadata(ident, name)?);
    }

    Ok(CanisterMetadata { tables })
}

/// Collects metadata for a database table from its name.
fn collect_table_metadata(table: Ident, name: String) -> syn::Result<TableMetadata> {
    let record_ident = Ident::new(&format!("{table}Record"), table.span());
    let insert_ident = Ident::new(&format!("{table}InsertRequest"), table.span());
    let update_ident = Ident::new(&format!("{table}UpdateRequest"), table.span());

    Ok(TableMetadata {
        table: table.clone(),
        record: record_ident,
        insert: insert_ident,
        update: update_ident,
        name,
    })
}
