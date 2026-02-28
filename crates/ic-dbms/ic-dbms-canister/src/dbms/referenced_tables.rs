use ic_dbms_api::prelude::ColumnDef;

type CacheMap = std::collections::HashMap<&'static str, Vec<(&'static str, Vec<&'static str>)>>;

thread_local! {
    /// Cache for referenced tables results.
    static CACHED_REFERENCED_TABLES: std::cell::RefCell<CacheMap> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Given a list of tables with their column definitions,
/// returns the list of tables that reference the target table.
///
/// The returned list contains tuples of table names and the columns
/// that reference the target table.
///
/// Example:
///
/// If we have the following tables:
/// - users (id)
/// - posts (id, user_id) where user_id references users.id
/// - comments (id, user_id) where user_id references user.id
///
/// Calling `get_referenced_tables("users", ...)` would return:
/// `[("posts", &["user_id"]), ("comments", &["user_id"])]`
pub fn get_referenced_tables(
    target: &'static str,
    tables: &[(&'static str, &'static [ColumnDef])],
) -> Vec<(&'static str, Vec<&'static str>)> {
    // check cache
    if let Some(cached) = CACHED_REFERENCED_TABLES.with_borrow(|cache| cache.get(target).cloned()) {
        return cached;
    }

    // compute referenced tables
    let referenced_tables = compute_referenced_tables(target, tables);
    CACHED_REFERENCED_TABLES.with_borrow_mut(|cache| {
        cache.insert(target, referenced_tables.clone());
    });
    referenced_tables
}

fn compute_referenced_tables(
    target: &'static str,
    tables: &[(&'static str, &'static [ColumnDef])],
) -> Vec<(&'static str, Vec<&'static str>)> {
    let mut referenced_tables = vec![];
    // iterate over tables
    for (table_name, columns) in tables {
        let mut referenced_tables_columns = vec![];
        // iterate over fks with target table as foreign table
        for fk in columns
            .iter()
            .filter_map(|col| col.foreign_key.as_ref())
            .filter(|fk| fk.foreign_table == target)
        {
            referenced_tables_columns.push(fk.local_column);
        }
        if !referenced_tables_columns.is_empty() {
            referenced_tables.push((*table_name, referenced_tables_columns));
        }
    }

    referenced_tables
}

#[cfg(test)]
mod tests {

    use ic_dbms_api::prelude::DataTypeKind;

    use super::*;
    use crate::prelude::TableSchema as _;
    use crate::tests::{Message, Post, User};

    #[test]
    fn test_should_get_referenced_tables() {
        let tables = &[
            (User::table_name(), User::columns()),
            (Post::table_name(), Post::columns()),
            (Message::table_name(), Message::columns()),
        ];
        let references = get_referenced_tables(User::table_name(), tables);
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].0, "posts");
        assert_eq!(references[0].1, vec!["user"]);
        assert_eq!(references[1].0, "messages");
        assert_eq!(references[1].1, vec!["sender", "recipient"]);
    }

    const SELF_REFERENCING_TABLE: &[ColumnDef] = &[
        ColumnDef {
            name: "id",
            data_type: DataTypeKind::Uint32,
            nullable: false,
            primary_key: true,
            foreign_key: None,
        },
        ColumnDef {
            name: "manager",
            data_type: DataTypeKind::Uint32,
            nullable: true,
            primary_key: false,
            foreign_key: Some(ic_dbms_api::prelude::ForeignKeyDef {
                local_column: "manager",
                foreign_table: "users",
                foreign_column: "id",
            }),
        },
    ];

    #[test]
    fn test_should_get_self_referenced_tables() {
        let tables = &[("users", SELF_REFERENCING_TABLE)];
        let references = get_referenced_tables("users", tables);
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].0, "users");
        assert_eq!(references[0].1, vec!["manager"]);
    }

    #[test]
    fn test_should_cache_referenced_tables() {
        let tables = &[
            (User::table_name(), User::columns()),
            (Post::table_name(), Post::columns()),
            (Message::table_name(), Message::columns()),
        ];
        // First call - should compute and cache
        let references1 = get_referenced_tables(User::table_name(), tables);
        let cached = CACHED_REFERENCED_TABLES
            .with_borrow(|cache| cache.get(User::table_name()).cloned())
            .expect("should be cached");
        assert_eq!(references1, cached);
    }
}
