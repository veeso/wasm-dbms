//! Test types, fixtures and mocks.

mod message;
mod post;
mod user;

use ic_dbms_api::prelude::{
    ColumnDef, Database as _, InsertRecord as _, Query, QueryError, TableSchema as _,
    UpdateRecord as _, Value, flatten_table_columns,
};

#[allow(unused_imports)]
pub use self::message::{
    MESSAGES_FIXTURES, Message, MessageInsertRequest, MessageRecord, MessageUpdateRequest,
};
#[allow(unused_imports)]
pub use self::post::{POSTS_FIXTURES, Post, PostInsertRequest, PostRecord, PostUpdateRequest};
#[allow(unused_imports)]
pub use self::user::{USERS_FIXTURES, User, UserInsertRequest, UserRecord, UserUpdateRequest};
use crate::dbms::IcDbmsDatabase;
use crate::prelude::{
    DatabaseSchema, InsertIntegrityValidator, UpdateIntegrityValidator, get_referenced_tables,
};

/// Loads fixtures into the database for testing purposes.
///
/// # Panics
///
/// Panics if any operation fails.
pub fn load_fixtures() {
    user::load_fixtures();
    post::load_fixtures();
    message::load_fixtures();
}

pub struct TestDatabaseSchema;

impl DatabaseSchema for TestDatabaseSchema {
    fn select(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &str,
        query: Query,
    ) -> ic_dbms_api::prelude::IcDbmsResult<Vec<Vec<(ColumnDef, Value)>>> {
        if table_name == User::table_name() {
            let results = dbms.select_columns::<User>(query)?;
            Ok(flatten_table_columns(results))
        } else if table_name == Post::table_name() {
            let results = dbms.select_columns::<Post>(query)?;
            Ok(flatten_table_columns(results))
        } else if table_name == Message::table_name() {
            let results = dbms.select_columns::<Message>(query)?;
            Ok(flatten_table_columns(results))
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn referenced_tables(&self, table: &'static str) -> Vec<(&'static str, Vec<&'static str>)> {
        let tables = &[
            (User::table_name(), User::columns()),
            (Post::table_name(), Post::columns()),
            (Message::table_name(), Message::columns()),
        ];
        get_referenced_tables(table, tables)
    }

    fn insert(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> ic_dbms_api::prelude::IcDbmsResult<()> {
        if table_name == User::table_name() {
            let insert_request = UserInsertRequest::from_values(record_values)?;
            dbms.insert::<User>(insert_request)
        } else if table_name == Post::table_name() {
            let insert_request = PostInsertRequest::from_values(record_values)?;
            dbms.insert::<Post>(insert_request)
        } else if table_name == Message::table_name() {
            let insert_request = MessageInsertRequest::from_values(record_values)?;
            dbms.insert::<Message>(insert_request)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn delete(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        delete_behavior: ic_dbms_api::prelude::DeleteBehavior,
        filter: Option<ic_dbms_api::prelude::Filter>,
    ) -> ic_dbms_api::prelude::IcDbmsResult<u64> {
        if table_name == User::table_name() {
            dbms.delete::<User>(delete_behavior, filter)
        } else if table_name == Post::table_name() {
            dbms.delete::<Post>(delete_behavior, filter)
        } else if table_name == Message::table_name() {
            dbms.delete::<Message>(delete_behavior, filter)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn update(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        patch_values: &[(ColumnDef, Value)],
        filter: Option<ic_dbms_api::prelude::Filter>,
    ) -> ic_dbms_api::prelude::IcDbmsResult<u64> {
        if table_name == User::table_name() {
            let update_request = UserUpdateRequest::from_values(patch_values, filter);
            dbms.update::<User>(update_request)
        } else if table_name == Post::table_name() {
            let update_request = PostUpdateRequest::from_values(patch_values, filter);
            dbms.update::<Post>(update_request)
        } else if table_name == Message::table_name() {
            let update_request = MessageUpdateRequest::from_values(patch_values, filter);
            dbms.update::<Message>(update_request)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn validate_insert(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> ic_dbms_api::prelude::IcDbmsResult<()> {
        if table_name == User::table_name() {
            InsertIntegrityValidator::<User>::new(dbms).validate(record_values)
        } else if table_name == Post::table_name() {
            InsertIntegrityValidator::<Post>::new(dbms).validate(record_values)
        } else if table_name == Message::table_name() {
            InsertIntegrityValidator::<Message>::new(dbms).validate(record_values)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }

    fn validate_update(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
        old_pk: Value,
    ) -> ic_dbms_api::prelude::IcDbmsResult<()> {
        if table_name == User::table_name() {
            UpdateIntegrityValidator::<User>::new(dbms, old_pk).validate(record_values)
        } else if table_name == Post::table_name() {
            UpdateIntegrityValidator::<Post>::new(dbms, old_pk).validate(record_values)
        } else if table_name == Message::table_name() {
            UpdateIntegrityValidator::<Message>::new(dbms, old_pk).validate(record_values)
        } else {
            Err(ic_dbms_api::prelude::IcDbmsError::Query(
                QueryError::TableNotFound(table_name.to_string()),
            ))
        }
    }
}
