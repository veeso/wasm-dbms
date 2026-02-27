#![crate_name = "ic_dbms_macros"]
#![crate_type = "lib"]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

//! Macros and derive for ic-dbms-canister
//!
//! This crate provides procedural macros to automatically implement traits
//! required by the `ic-dbms-canister`.
//!
//! ## Provided Derive Macros
//!
//! - `Encode`: Automatically implements the `Encode` trait for structs.
//! - `Table`: Automatically implements the `TableSchema` trait and associated types.
//! - `DbmsCanister`: Automatically implements the API for the ic-dbms-canister.
//!

#![doc(html_playground_url = "https://play.rust-lang.org")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/veeso/ic-dbms/main/assets/images/cargo/logo-128.png"
)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/veeso/ic-dbms/main/assets/images/cargo/logo-512.png"
)]

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod custom_data_type;
mod dbms_canister;
mod encode;
mod table;
mod utils;

/// Automatically implements the `Encode`` trait for a struct.
///
/// This derive macro generates two methods required by the `Encode` trait:
///
/// - `fn data_size() -> DataSize`  
///   Computes the static size of the encoded type.  
///   If all fields implement `Encode::data_size()` returning  
///   `DataSize::Fixed(n)`, then the type is also considered fixed-size.  
///   Otherwise, the type is `DataSize::Dynamic`.
///
/// - `fn size(&self) -> MSize`  
///   Computes the runtime-encoding size of the value by summing the
///   sizes of all fields.
///
/// # What the macro generates
///
/// Given a struct like:
///
/// ```rust,ignore
/// #[derive(Encode)]
/// struct User {
///     id: Uint32,
///     name: Text,
/// }
/// ```
///
/// The macro expands into:
///
/// ```rust,ignore
/// impl Encode for User {
///     const DATA_SIZE: DataSize = DataSize::Dynamic; // or DataSize::Fixed(n) if applicable
///
///     fn size(&self) -> MSize {
///         self.id.size() + self.name.size()
///     }
///
///     fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
///         let mut encoded = Vec::with_capacity(self.size() as usize);
///         encoded.extend_from_slice(&self.id.encode());
///         encoded.extend_from_slice(&self.name.encode());
///         std::borrow::Cow::Owned(encoded)
///     }
///
///     fn decode(data: std::borrow::Cow<[u8]>) -> ::ic_dbms_api::prelude::MemoryResult<Self> {
///         let mut offset = 0;
///         let id = Uint32::decode(std::borrow::Borrowed(&data[offset..]))?;
///         offset += id.size() as usize;
///         let name = Text::decode(std::borrow::Borrowed(&data[offset..]))?;
///         offset += name.size() as usize;
///         Ok(Self { id, name })
///     }
/// }
/// ```
/// # Requirements
///
/// - Each field type must implement `Encode`.
/// - Only works on `struct`s; enums and unions are not supported.
/// - All field identifiers must be valid Rust identifiers (no tuple structs).
///
/// # Notes
///
/// - It is intended for internal use within the `ic-dbms-canister` DBMS memory
///   system.
///
/// # Errors
///
/// The macro will fail to expand if:
///
/// - The struct has unnamed fields (tuple struct)
/// - A field type does not implement `Encode`
/// - The macro is applied to a non-struct item.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Encode, Debug, PartialEq, Eq)]
/// struct Position {
///     x: Int32,
///     y: Int32,
/// }
///
/// let pos = Position { x: 10.into(), y: 20.into() };
/// assert_eq!(Position::data_size(), DataSize::Fixed(8));
/// assert_eq!(pos.size(), 8);
/// let encoded = pos.encode();
/// let decoded = Position::decode(encoded).unwrap();
/// assert_eq!(pos, decoded);
/// ```
#[proc_macro_derive(Encode)]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    self::encode::encode(input, None)
        .expect("Failed to derive `Encode`")
        .into()
}

/// Given a struct representing a database table, automatically implements
/// the `TableSchema` trait with all the necessary types to work with the ic-dbms-canister.
/// So given this struct:
///
/// ```rust,ignore
/// #[derive(Table, Encode)]
/// #[table = "posts"]
/// struct Post {
///     #[primary_key]
///     id: Uint32,
///     title: Text,
///     content: Text,
///     #[foreign_key(entity = "User", table = "users", column = "id")]
///     author_id: Uint32,
/// }
/// ```
///
/// What we expect as output is:
///
/// - To implement the `TableSchema` trait for the struct as follows:
///
///     ```rust,ignore
///     impl TableSchema for Post {
///         type Insert = PostInsertRequest;
///         type Record = PostRecord;
///         type Update = PostUpdateRequest;
///         type ForeignFetcher = PostForeignFetcher;
///
///         fn columns() -> &'static [ColumnDef] {
///             &[
///                 ColumnDef {
///                     name: "id",
///                     data_type: DataTypeKind::Uint32,
///                     nullable: false,
///                     primary_key: true,
///                     foreign_key: None,
///                 },
///                 ColumnDef {
///                     name: "title",
///                     data_type: DataTypeKind::Text,
///                     nullable: false,
///                     primary_key: false,
///                     foreign_key: None,
///                 },
///                 ColumnDef {
///                     name: "content",
///                     data_type: DataTypeKind::Text,
///                     nullable: false,
///                     primary_key: false,
///                     foreign_key: None,
///                 },
///                 ColumnDef {
///                     name: "user_id",
///                     data_type: DataTypeKind::Uint32,
///                     nullable: false,
///                     primary_key: false,
///                     foreign_key: Some(ForeignKeyDef {
///                         local_column: "user_id",
///                         foreign_table: "users",
///                         foreign_column: "id",
///                     }),
///                 },
///             ]
///         }
///
///         fn table_name() -> &'static str {
///             "posts"
///         }
///
///         fn primary_key() -> &'static str {
///             "id"
///         }
///
///         fn to_values(self) -> Vec<(ColumnDef, Value)> {
///             vec![
///                 (Self::columns()[0], Value::Uint32(self.id)),
///                 (Self::columns()[1], Value::Text(self.title)),
///                 (Self::columns()[2], Value::Text(self.content)),
///                 (Self::columns()[3], Value::Uint32(self.user_id)),
///             ]
///         }
///     }
///     ```
///
/// - Implement the associated `Record` type
///
///     ```rust,ignore
///     pub struct PostRecord {
///         pub id: Option<Uint32>,
///         pub title: Option<Text>,
///         pub content: Option<Text>,
///         pub user: Option<UserRecord>,
///     }
///
///     impl TableRecord for PostRecord {
///         type Schema = Post;
///     
///         fn from_values(values: TableColumns) -> Self {
///             let mut id: Option<Uint32> = None;
///             let mut title: Option<Text> = None;
///             let mut content: Option<Text> = None;
///     
///             let post_values = values
///                 .iter()
///                 .find(|(table_name, _)| *table_name == ValuesSource::This)
///                 .map(|(_, cols)| cols);
///             
///             for (column, value) in post_values.unwrap_or(&vec![]) {
///                 match column.name {
///                     "id" => {
///                         if let Value::Uint32(v) = value {
///                             id = Some(*v);
///                         }
///                     }
///                     "title" => {
///                         if let Value::Text(v) = value {
///                             title = Some(v.clone());
///                         }
///                     }
///                     "content" => {
///                         if let Value::Text(v) = value {
///                             content = Some(v.clone());
///                         }
///                     }
///                     _ => { /* Ignore unknown columns */ }
///                 }
///             }
///     
///             let has_user = values.iter().any(|(source, _)| {
///                 *source
///                     == ValuesSource::Foreign {
///                         table: User::table_name(),
///                         column: "user_id",
///                     }
///             });
///             let user = if has_user {
///                 Some(UserRecord::from_values(self_reference_values(
///                     &values,
///                     User::table_name(),
///                     "user_id",
///                 )))
///             } else {
///                 None
///             };
///     
///             Self {
///                 id,
///                 title,
///                 content,
///                 user,
///             }
///         }
///     
///         fn to_values(&self) -> Vec<(ColumnDef, Value)> {
///             Self::Schema::columns()
///                 .iter()
///                 .zip(vec![
///                     match self.id {
///                         Some(v) => Value::Uint32(v),
///                         None => Value::Null,
///                     },
///                     match &self.title {
///                         Some(v) => Value::Text(v.clone()),
///                         None => Value::Null,
///                     },
///                     match &self.content {
///                         Some(v) => Value::Text(v.clone()),
///                         None => Value::Null,
///                     },
///                 ])
///                 .map(|(col_def, value)| (*col_def, value))
///                 .collect()
///         }
///     }
///     ```
///
/// - Implement the associated `InsertRecord` type
///
///     ```rust,ignore
///     #[derive(Clone)]
///     pub struct PostInsertRequest {
///         pub id: Uint32,
///         pub title: Text,
///         pub content: Text,
///         pub user_id: Uint32,
///     }
///
///     impl InsertRecord for PostInsertRequest {
///         type Record = PostRecord;
///         type Schema = Post;
///
///         fn from_values(values: &[(ColumnDef, Value)]) -> ic_dbms_api::prelude::IcDbmsResult<Self> {
///             let mut id: Option<Uint32> = None;
///             let mut title: Option<Text> = None;
///             let mut content: Option<Text> = None;
///             let mut user_id: Option<Uint32> = None;
///
///             for (column, value) in values {
///                 match column.name {
///                     "id" => {
///                         if let Value::Uint32(v) = value {
///                             id = Some(*v);
///                         }
///                     }
///                     "title" => {
///                         if let Value::Text(v) = value {
///                             title = Some(v.clone());
///                         }
///                     }
///                     "content" => {
///                         if let Value::Text(v) = value {
///                             content = Some(v.clone());
///                         }
///                     }
///                     "user_id" => {
///                         if let Value::Uint32(v) = value {
///                             user_id = Some(*v);
///                         }
///                     }
///                     _ => { /* Ignore unknown columns */ }
///                 }
///             }
///
///             Ok(Self {
///                 id: id.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
///                     "id".to_string(),
///                 )))?,
///                 title: title.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
///                     "title".to_string(),
///                 )))?,
///                 content: content.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
///                     "content".to_string(),
///                 )))?,
///                 user_id: user_id.ok_or(IcDbmsError::Query(QueryError::MissingNonNullableField(
///                     "user_id".to_string(),
///                 )))?,
///             })
///         }
///
///         fn into_values(self) -> Vec<(ColumnDef, Value)> {
///             vec![
///                 (Self::Schema::columns()[0], Value::Uint32(self.id)),
///                 (Self::Schema::columns()[1], Value::Text(self.title)),
///                 (Self::Schema::columns()[2], Value::Text(self.content)),
///                 (Self::Schema::columns()[3], Value::Uint32(self.user_id)),
///             ]
///         }
///
///         fn into_record(self) -> Self::Schema {
///             Post {
///                 id: self.id,
///                 title: self.title,
///                 content: self.content,
///                 user_id: self.user_id,
///             }
///         }
///     }
///     ```
///
/// - Implement the associated `UpdateRecord` type
///
///     ```rust,ignore
///     pub struct PostUpdateRequest {
///         pub id: Option<Uint32>,
///         pub title: Option<Text>,
///         pub content: Option<Text>,
///         pub user_id: Option<Uint32>,
///         pub where_clause: Option<Filter>,
///     }
///
///     impl UpdateRecord for PostUpdateRequest {
///         type Record = PostRecord;
///         type Schema = Post;
///
///         fn from_values(values: &[(ColumnDef, Value)], where_clause: Option<Filter>) -> Self {
///             let mut id: Option<Uint32> = None;
///             let mut title: Option<Text> = None;
///             let mut content: Option<Text> = None;
///             let mut user_id: Option<Uint32> = None;
///
///             for (column, value) in values {
///                 match column.name {
///                     "id" => {
///                         if let Value::Uint32(v) = value {
///                             id = Some(*v);
///                         }
///                     }
///                     "title" => {
///                         if let Value::Text(v) = value {
///                             title = Some(v.clone());
///                         }
///                     }
///                     "content" => {
///                         if let Value::Text(v) = value {
///                             content = Some(v.clone());
///                         }
///                     }
///                     "user_id" => {
///                         if let Value::Uint32(v) = value {
///                             user_id = Some(*v);
///                         }
///                     }
///                     _ => { /* Ignore unknown columns */ }
///                 }
///             }
///
///             Self {
///                 id,
///                 title,
///                 content,
///                 user_id,
///                 where_clause,
///             }
///         }
///
///         fn update_values(&self) -> Vec<(ColumnDef, Value)> {
///             let mut updates = Vec::new();
///
///             if let Some(id) = self.id {
///                 updates.push((Self::Schema::columns()[0], Value::Uint32(id)));
///             }
///             if let Some(title) = &self.title {
///                 updates.push((Self::Schema::columns()[1], Value::Text(title.clone())));
///             }
///             if let Some(content) = &self.content {
///                 updates.push((Self::Schema::columns()[2], Value::Text(content.clone())));
///             }
///             if let Some(user_id) = self.user_id {
///                 updates.push((Self::Schema::columns()[3], Value::Uint32(user_id)));
///             }
///
///             updates
///         }
///
///         fn where_clause(&self) -> Option<Filter> {
///             self.where_clause.clone()
///         }
///     }
///     ```
///
/// - If has foreign keys, implement the associated `ForeignFetched` (otherwise use `NoForeignFetcher`):
///
///     ```rust,ignore
///     pub struct PostForeignFetcher;
///
///     impl ForeignFetcher for PostForeignFetcher {
///         fn fetch(
///             &self,
///             database: &impl Database,
///             table: &'static str,
///             local_column: &'static str,
///             pk_value: Value,
///         ) -> ic_dbms_api::prelude::IcDbmsResult<TableColumns> {
///             if table != User::table_name() {
///                 return Err(IcDbmsError::Query(QueryError::InvalidQuery(format!(
///                     "ForeignFetcher: unknown table '{table}' for {table_name} foreign fetcher",
///                     table_name = Post::table_name()
///                 ))));
///             }
///
///             // query all records from the foreign table
///             let mut users = database.select(
///                 Query::<User>::builder()
///                     .all()
///                     .limit(1)
///                     .and_where(Filter::Eq(User::primary_key(), pk_value.clone()))
///                     .build(),
///             )?;
///             let user = match users.pop() {
///                 Some(user) => user,
///                 None => {
///                     return Err(IcDbmsError::Query(QueryError::BrokenForeignKeyReference {
///                         table: User::table_name(),
///                         key: pk_value,
///                     }));
///                 }
///             };
///
///             let values = user.to_values();
///             Ok(vec![(
///                 ValuesSource::Foreign {
///                     table,
///                     column: local_column,
///                 },
///                 values,
///             )])
///         }
///     }
///     ```
///
/// So for each struct deriving `Table`, we will generate the following type. Given `${StructName}`, we will generate:
///
/// - `${StructName}Record` - implementing `TableRecord`
/// - `${StructName}InsertRequest` - implementing `InsertRecord`
/// - `${StructName}UpdateRequest` - implementing `UpdateRecord`
/// - `${StructName}ForeignFetcher` (only if foreign keys are present)
///
/// Also, we will implement the `TableSchema` trait for the struct itself and derive `Encode` for `${StructName}`.
///
/// ## Attributes
///
/// The `Table` derive macro supports the following attributes:
///
/// - `#[table = "table_name"]`: Specifies the name of the table in the database.
/// - `#[alignment = N]`: (optional) Specifies the alignment for the table records. Use only if you know what you are doing.
/// - `#[primary_key]`: Marks a field as the primary key of the table.
/// - `#[foreign_key(entity = "EntityName", table = "table_name", column = "column_name")]`: Defines a foreign key relationship.
/// - `#[sanitizer(SanitizerType)]`: Specifies a sanitize for the field.
/// - `#[validate(ValidatorType)]`: Specifies a validator for the field.
///
#[proc_macro_derive(
    Table,
    attributes(
        alignment,
        table,
        primary_key,
        foreign_key,
        sanitizer,
        validate,
        custom_type
    )
)]
pub fn derive_table(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    self::table::table(input)
        .expect("failed to derive `Table`")
        .into()
}

/// Automatically implements the api for the ic-dbms-canister with all the required methods to interact with the ACL and
/// the defined tables.
#[proc_macro_derive(DbmsCanister, attributes(tables))]
pub fn derive_dbms_canister(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    self::dbms_canister::dbms_canister(input)
        .expect("failed to derive `DbmsCanister`")
        .into()
}

/// Derives the [`CustomDataType`] trait and an `impl From<T> for Value` conversion
/// for a user-defined enum or struct.
///
/// The type must also derive [`Encode`] (for binary serialization) and implement
/// [`Display`](std::fmt::Display) (for the cached display string in [`CustomValue`]).
///
/// # Required attribute
///
/// - `#[type_tag = "..."]`: A unique string identifier for this custom data type.
///
/// # What the macro generates
///
/// Given a type like:
///
/// ```rust,ignore
/// #[derive(Encode, CustomDataType)]
/// #[type_tag = "status"]
/// enum Status { Active, Inactive }
/// ```
///
/// The macro expands into:
///
/// ```rust,ignore
/// impl CustomDataType for Status {
///     const TYPE_TAG: &'static str = "status";
/// }
///
/// impl From<Status> for Value {
///     fn from(val: Status) -> Value {
///         Value::Custom(CustomValue {
///             type_tag: "status".to_string(),
///             encoded: Encode::encode(&val).into_owned(),
///             display: val.to_string(),
///         })
///     }
/// }
/// ```
///
/// # Note
///
/// The user must also provide `Display`, `Default`, and `DataType` implementations
/// for the type. This macro only bridges the custom type to the `Value` system.
#[proc_macro_derive(CustomDataType, attributes(type_tag))]
pub fn derive_custom_data_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    custom_data_type::custom_data_type(&input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
