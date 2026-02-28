// Rust guideline compliant 2026-02-28

//! Example table schemas and [`DatabaseSchema`] implementation.
//!
//! Defines `User` and `Post` tables along with a concrete
//! [`ExampleDatabaseSchema`] that dispatches generic DBMS
//! operations to the correct table type.

use wasm_dbms::prelude::{
    DatabaseSchema, DbmsContext, InsertIntegrityValidator, UpdateIntegrityValidator,
    WasmDbmsDatabase, get_referenced_tables,
};
use wasm_dbms_api::prelude::{
    ColumnDef, Database as _, DbmsError, DbmsResult, DeleteBehavior, Filter, InsertRecord as _,
    MaxStrlenValidator, Query, QueryError, TableSchema as _, Text, TrimSanitizer, Uint32,
    UpdateRecord as _, Value, flatten_table_columns,
};
use wasm_dbms_macros::Table;
use wasm_dbms_memory::prelude::MemoryProvider;

// ---------- Table definitions ----------

/// Users table.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    /// Primary key identifier.
    #[primary_key]
    pub id: Uint32,
    /// Display name (trimmed, max 20 characters).
    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(20))]
    pub name: Text,
    /// Email address.
    pub email: Text,
}

/// Posts table with a foreign key referencing `users`.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    /// Primary key identifier.
    #[primary_key]
    pub id: Uint32,
    /// Post title.
    pub title: Text,
    /// Post content body.
    pub content: Text,
    /// Foreign key to the owning user.
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub user: Uint32,
}

// ---------- DatabaseSchema implementation ----------

/// Schema implementation that dispatches operations to the
/// `User` and `Post` table types.
#[derive(Debug)]
pub struct ExampleDatabaseSchema;

impl<M: MemoryProvider> DatabaseSchema<M> for ExampleDatabaseSchema {
    fn select(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &str,
        query: Query,
    ) -> DbmsResult<Vec<Vec<(ColumnDef, Value)>>> {
        if table_name == User::table_name() {
            let results = dbms.select_columns::<User>(query)?;
            Ok(flatten_table_columns(results))
        } else if table_name == Post::table_name() {
            let results = dbms.select_columns::<Post>(query)?;
            Ok(flatten_table_columns(results))
        } else {
            Err(DbmsError::Query(QueryError::TableNotFound(
                table_name.to_string(),
            )))
        }
    }

    fn referenced_tables(&self, table: &'static str) -> Vec<(&'static str, Vec<&'static str>)> {
        let tables = &[
            (User::table_name(), User::columns()),
            (Post::table_name(), Post::columns()),
        ];
        get_referenced_tables(table, tables)
    }

    fn insert(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> DbmsResult<()> {
        if table_name == User::table_name() {
            let insert = UserInsertRequest::from_values(record_values)?;
            dbms.insert::<User>(insert)
        } else if table_name == Post::table_name() {
            let insert = PostInsertRequest::from_values(record_values)?;
            dbms.insert::<Post>(insert)
        } else {
            Err(DbmsError::Query(QueryError::TableNotFound(
                table_name.to_string(),
            )))
        }
    }

    fn delete(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        delete_behavior: DeleteBehavior,
        filter: Option<Filter>,
    ) -> DbmsResult<u64> {
        if table_name == User::table_name() {
            dbms.delete::<User>(delete_behavior, filter)
        } else if table_name == Post::table_name() {
            dbms.delete::<Post>(delete_behavior, filter)
        } else {
            Err(DbmsError::Query(QueryError::TableNotFound(
                table_name.to_string(),
            )))
        }
    }

    fn update(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        patch_values: &[(ColumnDef, Value)],
        filter: Option<Filter>,
    ) -> DbmsResult<u64> {
        if table_name == User::table_name() {
            let update = UserUpdateRequest::from_values(patch_values, filter);
            dbms.update::<User>(update)
        } else if table_name == Post::table_name() {
            let update = PostUpdateRequest::from_values(patch_values, filter);
            dbms.update::<Post>(update)
        } else {
            Err(DbmsError::Query(QueryError::TableNotFound(
                table_name.to_string(),
            )))
        }
    }

    fn validate_insert(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> DbmsResult<()> {
        if table_name == User::table_name() {
            InsertIntegrityValidator::<User, M>::new(dbms).validate(record_values)
        } else if table_name == Post::table_name() {
            InsertIntegrityValidator::<Post, M>::new(dbms).validate(record_values)
        } else {
            Err(DbmsError::Query(QueryError::TableNotFound(
                table_name.to_string(),
            )))
        }
    }

    fn validate_update(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
        old_pk: Value,
    ) -> DbmsResult<()> {
        if table_name == User::table_name() {
            UpdateIntegrityValidator::<User, M>::new(dbms, old_pk).validate(record_values)
        } else if table_name == Post::table_name() {
            UpdateIntegrityValidator::<Post, M>::new(dbms, old_pk).validate(record_values)
        } else {
            Err(DbmsError::Query(QueryError::TableNotFound(
                table_name.to_string(),
            )))
        }
    }
}

/// Registers all example tables in the given DBMS context.
pub fn register_tables<M: MemoryProvider>(ctx: &DbmsContext<M>) -> DbmsResult<()> {
    ctx.register_table::<User>()?;
    ctx.register_table::<Post>()?;
    Ok(())
}
