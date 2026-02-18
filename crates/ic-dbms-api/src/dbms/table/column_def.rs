use candid::CandidType;
use serde::{Deserialize, Serialize};

use crate::dbms::types::DataTypeKind;

/// Defines a column in a database table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnDef {
    /// The name of the column.
    pub name: &'static str,
    /// The data type of the column.
    pub data_type: DataTypeKind,
    /// Indicates if this column can contain NULL values.
    pub nullable: bool,
    /// Indicates if this column is part of the primary key.
    pub primary_key: bool,
    /// Foreign key definition, if any.
    pub foreign_key: Option<ForeignKeyDef>,
}

/// Defines a foreign key relationship for a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForeignKeyDef {
    /// Name of the local column that holds the foreign key (es: "user_id")
    pub local_column: &'static str,
    /// Name of the foreign table (e.g., "users")
    pub foreign_table: &'static str,
    /// Name of the foreign column that the FK points to (e.g., "id")
    pub foreign_column: &'static str,
}

/// Candid-serializable column definition for canister API boundaries.
///
/// This type mirrors [`ColumnDef`] but uses owned `String` fields instead
/// of `&'static str`, making it compatible with `CandidType` serialization.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Serialize, Deserialize)]
pub struct CandidColumnDef {
    /// The source table name. `Some` for join results, `None` for single-table queries.
    pub table: Option<String>,
    /// The name of the column.
    pub name: String,
    /// The data type of the column.
    pub data_type: DataTypeKind,
    /// Indicates if this column can contain NULL values.
    pub nullable: bool,
    /// Indicates if this column is part of the primary key.
    pub primary_key: bool,
    /// Foreign key definition, if any.
    pub foreign_key: Option<CandidForeignKeyDef>,
}

/// Candid-serializable foreign key definition for canister API boundaries.
///
/// This type mirrors [`ForeignKeyDef`] but uses owned `String` fields instead
/// of `&'static str`, making it compatible with `CandidType` serialization.
#[derive(Clone, Debug, PartialEq, Eq, CandidType, Serialize, Deserialize)]
pub struct CandidForeignKeyDef {
    /// Name of the local column that holds the foreign key (e.g., "user_id").
    pub local_column: String,
    /// Name of the foreign table (e.g., "users").
    pub foreign_table: String,
    /// Name of the foreign column that the FK points to (e.g., "id").
    pub foreign_column: String,
}

impl From<ColumnDef> for CandidColumnDef {
    fn from(def: ColumnDef) -> Self {
        Self {
            table: None,
            name: def.name.to_string(),
            data_type: def.data_type,
            nullable: def.nullable,
            primary_key: def.primary_key,
            foreign_key: def.foreign_key.map(CandidForeignKeyDef::from),
        }
    }
}

impl From<ForeignKeyDef> for CandidForeignKeyDef {
    fn from(def: ForeignKeyDef) -> Self {
        Self {
            local_column: def.local_column.to_string(),
            foreign_table: def.foreign_table.to_string(),
            foreign_column: def.foreign_column.to_string(),
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use crate::dbms::types::DataTypeKind;

    #[test]
    fn test_should_create_column_def() {
        let column = ColumnDef {
            name: "id",
            data_type: DataTypeKind::Uint32,
            nullable: false,
            primary_key: true,
            foreign_key: None,
        };

        assert_eq!(column.name, "id");
        assert_eq!(column.data_type, DataTypeKind::Uint32);
        assert!(!column.nullable);
        assert!(column.primary_key);
        assert!(column.foreign_key.is_none());
    }

    #[test]
    fn test_should_create_column_def_with_foreign_key() {
        let fk = ForeignKeyDef {
            local_column: "user_id",
            foreign_table: "users",
            foreign_column: "id",
        };

        let column = ColumnDef {
            name: "user_id",
            data_type: DataTypeKind::Uint32,
            nullable: false,
            primary_key: false,
            foreign_key: Some(fk),
        };

        assert_eq!(column.name, "user_id");
        assert!(column.foreign_key.is_some());
        let fk_def = column.foreign_key.unwrap();
        assert_eq!(fk_def.local_column, "user_id");
        assert_eq!(fk_def.foreign_table, "users");
        assert_eq!(fk_def.foreign_column, "id");
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_should_clone_column_def() {
        let column = ColumnDef {
            name: "email",
            data_type: DataTypeKind::Text,
            nullable: true,
            primary_key: false,
            foreign_key: None,
        };

        let cloned = column.clone();
        assert_eq!(column, cloned);
    }

    #[test]
    fn test_should_compare_column_defs() {
        let column1 = ColumnDef {
            name: "id",
            data_type: DataTypeKind::Uint32,
            nullable: false,
            primary_key: true,
            foreign_key: None,
        };

        let column2 = ColumnDef {
            name: "id",
            data_type: DataTypeKind::Uint32,
            nullable: false,
            primary_key: true,
            foreign_key: None,
        };

        let column3 = ColumnDef {
            name: "name",
            data_type: DataTypeKind::Text,
            nullable: true,
            primary_key: false,
            foreign_key: None,
        };

        assert_eq!(column1, column2);
        assert_ne!(column1, column3);
    }

    #[test]
    fn test_should_create_foreign_key_def() {
        let fk = ForeignKeyDef {
            local_column: "post_id",
            foreign_table: "posts",
            foreign_column: "id",
        };

        assert_eq!(fk.local_column, "post_id");
        assert_eq!(fk.foreign_table, "posts");
        assert_eq!(fk.foreign_column, "id");
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_should_clone_foreign_key_def() {
        let fk = ForeignKeyDef {
            local_column: "author_id",
            foreign_table: "authors",
            foreign_column: "id",
        };

        let cloned = fk.clone();
        assert_eq!(fk, cloned);
    }

    #[test]
    fn test_should_compare_foreign_key_defs() {
        let fk1 = ForeignKeyDef {
            local_column: "user_id",
            foreign_table: "users",
            foreign_column: "id",
        };

        let fk2 = ForeignKeyDef {
            local_column: "user_id",
            foreign_table: "users",
            foreign_column: "id",
        };

        let fk3 = ForeignKeyDef {
            local_column: "category_id",
            foreign_table: "categories",
            foreign_column: "id",
        };

        assert_eq!(fk1, fk2);
        assert_ne!(fk1, fk3);
    }

    #[test]
    fn test_should_create_candid_column_def_with_table() {
        let col = CandidColumnDef {
            table: Some("users".to_string()),
            name: "id".to_string(),
            data_type: DataTypeKind::Uint32,
            nullable: false,
            primary_key: true,
            foreign_key: None,
        };
        assert_eq!(col.table, Some("users".to_string()));
    }

    #[test]
    fn test_should_convert_column_def_to_candid_with_none_table() {
        let col = ColumnDef {
            name: "id",
            data_type: DataTypeKind::Uint32,
            nullable: false,
            primary_key: true,
            foreign_key: None,
        };
        let candid_col = CandidColumnDef::from(col);
        assert_eq!(candid_col.table, None);
        assert_eq!(candid_col.name, "id");
    }
}
