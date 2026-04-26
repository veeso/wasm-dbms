# Relationships

- [Relationships](#relationships)
  - [Overview](#overview)
  - [Defining Foreign Keys](#defining-foreign-keys)
    - [Foreign Key Syntax](#foreign-key-syntax)
    - [Foreign Key Constraints](#foreign-key-constraints)
  - [Referential Integrity](#referential-integrity)
    - [Insert Validation](#insert-validation)
    - [Update Validation](#update-validation)
  - [Delete Behaviors](#delete-behaviors)
    - [Restrict](#restrict)
    - [Cascade](#cascade)
    - [Choosing a Delete Behavior](#choosing-a-delete-behavior)
  - [Eager Loading](#eager-loading)
    - [Basic Eager Loading](#basic-eager-loading)
    - [Multiple Relations](#multiple-relations)
    - [Eager Loading with Filters](#eager-loading-with-filters)
    - [Cross-Table Queries with Joins](#cross-table-queries-with-joins)
  - [Common Patterns](#common-patterns)
    - [One-to-Many](#one-to-many)
    - [Many-to-Many](#many-to-many)
    - [Self-Referential](#self-referential)

---

## Overview

wasm-dbms supports foreign key relationships between tables, providing:

- **Referential integrity**: Ensures foreign keys point to valid records
- **Delete behaviors**: Control what happens when referenced records are deleted
- **Eager loading**: Load related records in a single query

---

## Defining Foreign Keys

### Foreign Key Syntax

Use the `#[foreign_key]` attribute to define relationships:

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    pub content: Text,

    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,
}
```

**Attribute parameters:**

| Parameter | Description                                                  |
| --------- | ------------------------------------------------------------ |
| `entity`  | The Rust struct name of the referenced table                 |
| `table`   | The table name (as specified in `#[table = "..."]`)          |
| `column`  | The column in the referenced table (usually the primary key) |

### Foreign Key Constraints

When you define a foreign key:

1. The field type must match the referenced column type
2. The referenced table must be registered in your database schema
3. Foreign key values must reference existing records (enforced on insert/update)

---

## Referential Integrity

wasm-dbms enforces referential integrity automatically.

### Insert Validation

When inserting a record with a foreign key, the referenced record must exist:

```rust
// This user exists
database.insert::<User>(UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    ..
})?;

// Insert post referencing existing user - OK
database.insert::<Post>(PostInsertRequest {
    id: 1.into(),
    title: "My Post".into(),
    author_id: 1.into(),  // User 1 exists
    ..
})?;

// Insert post referencing non-existent user - FAILS
let result = database.insert::<Post>(PostInsertRequest {
    id: 2.into(),
    title: "Another Post".into(),
    author_id: 999.into(),  // User 999 doesn't exist
    ..
});

assert!(matches!(
    result,
    Err(DbmsError::Query(QueryError::BrokenForeignKeyReference))
));
```

### Update Validation

Updates are also validated:

```rust
// Changing author_id to non-existent user fails
let update = PostUpdateRequest::builder()
    .set_author_id(999.into())  // User 999 doesn't exist
    .filter(Filter::eq("id", Value::Uint32(1.into())))
    .build();

let result = database.update::<Post>(update);
assert!(matches!(
    result,
    Err(DbmsError::Query(QueryError::BrokenForeignKeyReference))
));
```

---

## Delete Behaviors

When deleting a record that is referenced by other records, you must specify how to handle the references.

### Restrict

**Behavior**: Fail if any records reference this one.

```rust
use wasm_dbms_api::prelude::DeleteBehavior;

// User has posts - delete fails
let result = database.delete::<User>(
    DeleteBehavior::Restrict,
    Some(Filter::eq("id", Value::Uint32(1.into()))),
);

match result {
    Err(DbmsError::Query(QueryError::ForeignKeyConstraintViolation)) => {
        println!("Cannot delete: user has posts");
    }
    _ => {}
}

// Delete posts first, then user
database.delete::<Post>(
    DeleteBehavior::Restrict,
    Some(Filter::eq("author_id", Value::Uint32(1.into()))),
)?;

// Now user can be deleted
database.delete::<User>(
    DeleteBehavior::Restrict,
    Some(Filter::eq("id", Value::Uint32(1.into()))),
)?;
```

**Use when**: You want to prevent accidental data loss. The caller must explicitly handle related records.

### Cascade

**Behavior**: Delete all records that reference this one (recursively).

```rust
// Deletes user AND all their posts
database.delete::<User>(
    DeleteBehavior::Cascade,
    Some(Filter::eq("id", Value::Uint32(1.into()))),
)?;
```

**Cascade is recursive:**

```rust
// Schema:
// User -> Posts -> Comments
// Deleting a user cascades to posts, which cascades to comments

database.delete::<User>(
    DeleteBehavior::Cascade,
    Some(Filter::eq("id", Value::Uint32(1.into()))),
)?;
// User deleted
// All user's posts deleted
// All comments on those posts deleted
```

**Use when**: Related records have no meaning without the parent (e.g., comments on a deleted post).

### Choosing a Delete Behavior

| Scenario                                  | Recommended Behavior                          |
| ----------------------------------------- | --------------------------------------------- |
| User account deletion (remove everything) | `Cascade`                                     |
| Prevent accidental deletion               | `Restrict`                                    |
| Soft delete pattern                       | Don't delete; use status field                |
| Comments on posts                         | `Cascade` (comments meaningless without post) |
| Products in orders                        | `Restrict` (orders are historical records)    |

---

## Eager Loading

Eager loading fetches related records in a single query, avoiding N+1 query problems.

### Basic Eager Loading

Use `.with()` to eager load a related table:

```rust
// Load posts with their authors
let query = Query::builder()
    .all()
    .with("users")  // Name of the related table
    .build();

let posts = database.select::<Post>(query)?;

// Each post now has author data available
for post in posts {
    println!("Post '{}' by author_id {}", post.title, post.author_id);
}
```

### Multiple Relations

Load multiple related tables:

```rust
// Schema:
// Post -> User (author)
// Post -> Category

let query = Query::builder()
    .all()
    .with("users")
    .with("categories")
    .build();

let posts = database.select::<Post>(query)?;
```

### Eager Loading with Filters

Combine eager loading with filters:

```rust
// Load published posts with their authors
let query = Query::builder()
    .filter(Filter::eq("published", Value::Boolean(true)))
    .order_by("created_at", OrderDirection::Descending)
    .limit(10)
    .with("users")
    .build();

let posts = database.select::<Post>(query)?;
```

### Cross-Table Queries with Joins

In addition to eager loading, wasm-dbms supports SQL-style joins (INNER, LEFT, RIGHT, FULL) for combining rows from multiple tables into a flat result set. Joins are useful when you need columns from several tables in a single row -- for example, listing post titles alongside author names. Unlike eager loading, joins return untyped results via the `select_raw` path.

See the [Querying Guide -- Joins](./querying.md#joins) section for full details and examples.

---

## Common Patterns

### One-to-Many

A user has many posts:

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,
}

// Query all posts by a user
let query = Query::builder()
    .filter(Filter::eq("author_id", Value::Uint32(user_id.into())))
    .build();
let user_posts = database.select::<Post>(query)?;
```

### Many-to-Many

Use a junction table for many-to-many relationships:

```rust
// Students and Courses (many-to-many)

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "students"]
pub struct Student {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "courses"]
pub struct Course {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "enrollments"]
pub struct Enrollment {
    #[primary_key]
    pub id: Uint32,
    #[foreign_key(entity = "Student", table = "students", column = "id")]
    pub student_id: Uint32,
    #[foreign_key(entity = "Course", table = "courses", column = "id")]
    pub course_id: Uint32,
    pub enrolled_at: DateTime,
}

// Find all courses for a student
let query = Query::builder()
    .filter(Filter::eq("student_id", Value::Uint32(student_id.into())))
    .with("courses")
    .build();
let enrollments = database.select::<Enrollment>(query)?;
```

### Self-Referential

A table can reference itself (e.g., categories with parent categories, employees with managers):

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "employees"]
pub struct Employee {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    #[foreign_key(entity = "Employee", table = "employees", column = "id")]
    pub manager_id: Nullable<Uint32>,  // Nullable for top-level employees
}

// Find all employees under a manager
let query = Query::builder()
    .filter(Filter::eq("manager_id", Value::Uint32(manager_id.into())))
    .build();
let direct_reports = database.select::<Employee>(query)?;
```

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "categories"]
pub struct Category {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    #[foreign_key(entity = "Category", table = "categories", column = "id")]
    pub parent_id: Nullable<Uint32>,  // Nullable for root categories
}

// Find root categories
let query = Query::builder()
    .filter(Filter::is_null("parent_id"))
    .build();
let root_categories = database.select::<Category>(query)?;

// Find children of a category
let query = Query::builder()
    .filter(Filter::eq("parent_id", Value::Uint32(parent_id.into())))
    .build();
let children = database.select::<Category>(query)?;
```
