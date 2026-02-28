# Validation Reference

- [Validation Reference](#validation-reference)
  - [Overview](#overview)
  - [Syntax](#syntax)
  - [Built-in Validators](#built-in-validators)
    - [String Length Validators](#string-length-validators)
    - [Format Validators](#format-validators)
    - [Case Validators](#case-validators)
    - [Locale Validators](#locale-validators)
  - [Implementing Custom Validators](#implementing-custom-validators)
  - [Validation Errors](#validation-errors)
  - [Examples](#examples)

---

## Overview

Validators enforce constraints on data being inserted or updated. If validation fails, the operation is rejected with a `Validation` error.

**Key points:**

- Validators run after sanitizers
- Validation failure rejects the entire operation
- Multiple validators can be applied to a single field
- Validators are applied on both insert and update

---

## Syntax

The `#[validate(...)]` attribute adds validation rules to fields:

```rust
use wasm_dbms_api::prelude::*;

#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    // Unit struct validator (no parameters)
    #[validate(EmailValidator)]
    pub email: Text,

    // Tuple struct validator (positional parameter)
    #[validate(MaxStrlenValidator(100))]
    pub name: Text,
}
```

---

## Built-in Validators

All validators are available in `wasm_dbms_api::prelude`.

### String Length Validators

**MaxStrlenValidator** - Maximum string length

```rust
#[validate(MaxStrlenValidator(255))]
pub description: Text,  // Max 255 characters
```

**MinStrlenValidator** - Minimum string length

```rust
#[validate(MinStrlenValidator(8))]
pub password: Text,  // At least 8 characters
```

**RangeStrlenValidator** - String length within range

```rust
#[validate(RangeStrlenValidator(3, 50))]
pub username: Text,  // Between 3 and 50 characters
```

### Format Validators

**EmailValidator** - Valid email format

```rust
#[validate(EmailValidator)]
pub email: Text,  // Must be valid email
```

**UrlValidator** - Valid URL format

```rust
#[validate(UrlValidator)]
pub website: Text,  // Must be valid URL
```

**PhoneNumberValidator** - Valid phone number format

```rust
#[validate(PhoneNumberValidator)]
pub phone: Text,  // Must be valid phone number
```

**MimeTypeValidator** - Valid MIME type format

```rust
#[validate(MimeTypeValidator)]
pub content_type: Text,  // e.g., "application/json", "image/png"
```

**RgbColorValidator** - Valid RGB color format

```rust
#[validate(RgbColorValidator)]
pub color: Text,  // e.g., "#FF5733", "rgb(255, 87, 51)"
```

### Case Validators

**CamelCaseValidator** - Must be camelCase

```rust
#[validate(CamelCaseValidator)]
pub identifier: Text,  // e.g., "myVariableName"
```

**KebabCaseValidator** - Must be kebab-case

```rust
#[validate(KebabCaseValidator)]
pub slug: Text,  // e.g., "my-page-slug"
```

**SnakeCaseValidator** - Must be snake_case

```rust
#[validate(SnakeCaseValidator)]
pub code: Text,  // e.g., "my_constant_name"
```

### Locale Validators

**CountryIso639Validator** - ISO 639 language code

```rust
#[validate(CountryIso639Validator)]
pub language: Text,  // e.g., "en", "es", "fr"
```

**CountryIso3166Validator** - ISO 3166 country code

```rust
#[validate(CountryIso3166Validator)]
pub country: Text,  // e.g., "US", "GB", "DE"
```

---

## Implementing Custom Validators

Create a struct implementing the `Validate` trait:

```rust
use wasm_dbms_api::prelude::{Validate, Value, DbmsResult, DbmsError};

/// Validates that a number is positive
pub struct PositiveValidator;

impl Validate for PositiveValidator {
    fn validate(&self, value: &Value) -> DbmsResult<()> {
        match value {
            Value::Int32(n) if n.0 > 0 => Ok(()),
            Value::Int64(n) if n.0 > 0 => Ok(()),
            Value::Decimal(d) if d.0 > rust_decimal::Decimal::ZERO => Ok(()),
            Value::Int32(_) | Value::Int64(_) | Value::Decimal(_) => {
                Err(DbmsError::Validation("Value must be positive".to_string()))
            }
            _ => Err(DbmsError::Validation("PositiveValidator only applies to numeric types".to_string()))
        }
    }
}

// Usage
#[derive(Table, ...)]
#[table = "products"]
pub struct Product {
    #[primary_key]
    pub id: Uint32,
    #[validate(PositiveValidator)]
    pub price: Decimal,
}
```

**Custom validator with parameters (tuple struct):**

```rust
/// Validates that a string matches a regex pattern
pub struct RegexValidator(pub &'static str);

impl Validate for RegexValidator {
    fn validate(&self, value: &Value) -> DbmsResult<()> {
        if let Value::Text(text) = value {
            let re = regex::Regex::new(self.0).unwrap();
            if re.is_match(text.as_str()) {
                return Ok(());
            }
        }
        Err(DbmsError::Validation(
            format!("Value does not match pattern: {}", self.0)
        ))
    }
}

// Usage
#[validate(RegexValidator(r"^[A-Z]{2}-\d{4}$"))]
pub product_code: Text,  // Must match "XX-1234" format
```

**Custom validator with named parameters:**

```rust
/// Validates a number is within a range
pub struct RangeValidator {
    pub min: i64,
    pub max: i64,
}

impl Validate for RangeValidator {
    fn validate(&self, value: &Value) -> DbmsResult<()> {
        let num = match value {
            Value::Int32(n) => n.0 as i64,
            Value::Int64(n) => n.0,
            _ => return Err(DbmsError::Validation("RangeValidator requires integer".to_string())),
        };

        if num >= self.min && num <= self.max {
            Ok(())
        } else {
            Err(DbmsError::Validation(
                format!("Value must be between {} and {}", self.min, self.max)
            ))
        }
    }
}

// Usage
#[validate(RangeValidator, min = 1, max = 100)]
pub percentage: Int32,
```

---

## Validation Errors

When validation fails, a `DbmsError::Validation(String)` is returned:

```rust
let result = database.insert::<User>(user);

match result {
    Ok(()) => println!("Insert successful"),
    Err(DbmsError::Validation(msg)) => {
        println!("Validation failed: {}", msg);
        // e.g., "Invalid email format"
        // e.g., "String length exceeds maximum of 100"
    }
    Err(e) => println!("Other error: {:?}", e),
}
```

---

## Examples

**Comprehensive user validation:**

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[validate(RangeStrlenValidator(2, 50))]
    pub name: Text,

    #[validate(EmailValidator)]
    pub email: Text,

    #[validate(MinStrlenValidator(8))]
    pub password_hash: Text,

    #[validate(PhoneNumberValidator)]
    pub phone: Nullable<Text>,

    #[validate(UrlValidator)]
    pub website: Nullable<Text>,

    #[validate(CountryIso3166Validator)]
    pub country: Nullable<Text>,

    #[validate(CountryIso639Validator)]
    pub language: Text,
}
```

**Product validation:**

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "products"]
pub struct Product {
    #[primary_key]
    pub id: Uuid,

    #[validate(RangeStrlenValidator(1, 200))]
    pub name: Text,

    #[validate(MaxStrlenValidator(2000))]
    pub description: Text,

    #[validate(KebabCaseValidator)]
    pub slug: Text,

    #[validate(MimeTypeValidator)]
    pub image_type: Nullable<Text>,

    #[validate(RgbColorValidator)]
    pub accent_color: Nullable<Text>,
}
```

**Combined with sanitizers:**

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "articles"]
pub struct Article {
    #[primary_key]
    pub id: Uuid,

    // Sanitize first, then validate
    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(200))]
    pub title: Text,

    // Convert to slug format, then validate
    #[sanitizer(SlugSanitizer)]
    #[validate(KebabCaseValidator)]
    pub slug: Text,

    #[sanitizer(TrimSanitizer)]
    pub content: Text,
}
```
