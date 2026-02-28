# Sanitization Reference

- [Overview](#overview)
- [Syntax](#syntax)
- [Built-in Sanitizers](#built-in-sanitizers)
  - [String Sanitizers](#string-sanitizers)
  - [Numeric Sanitizers](#numeric-sanitizers)
  - [DateTime Sanitizers](#datetime-sanitizers)
  - [Null Sanitizers](#null-sanitizers)
- [Implementing Custom Sanitizers](#implementing-custom-sanitizers)
- [Sanitization Order](#sanitization-order)
- [Examples](#examples)

---

## Overview

Sanitizers automatically transform data before it's stored in the database. Unlike validators (which reject invalid data), sanitizers modify data to conform to expected formats.

**Key points:**
- Sanitizers run before validators
- Data is transformed, not rejected
- Multiple sanitizers can be chained
- Sanitizers apply on both insert and update

---

## Syntax

The `#[sanitizer(...)]` attribute adds sanitization rules to fields:

```rust
use wasm_dbms_api::prelude::*;

#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    // Unit struct sanitizer (no parameters)
    #[sanitizer(TrimSanitizer)]
    pub name: Text,

    // Tuple struct sanitizer (positional parameter)
    #[sanitizer(RoundToScaleSanitizer(2))]
    pub balance: Decimal,

    // Named fields sanitizer
    #[sanitizer(ClampSanitizer, min = 0, max = 120)]
    pub age: Uint8,
}
```

---

## Built-in Sanitizers

All sanitizers are available in `wasm_dbms_api::prelude`.

### String Sanitizers

**TrimSanitizer** - Remove leading/trailing whitespace

```rust
#[sanitizer(TrimSanitizer)]
pub name: Text,
// "  Alice  " → "Alice"
```

**CollapseWhitespaceSanitizer** - Collapse multiple spaces into one

```rust
#[sanitizer(CollapseWhitespaceSanitizer)]
pub description: Text,
// "Hello    World" → "Hello World"
```

**LowerCaseSanitizer** - Convert to lowercase

```rust
#[sanitizer(LowerCaseSanitizer)]
pub email: Text,
// "Alice@Example.COM" → "alice@example.com"
```

**UpperCaseSanitizer** - Convert to uppercase

```rust
#[sanitizer(UpperCaseSanitizer)]
pub country_code: Text,
// "us" → "US"
```

**SlugSanitizer** - Convert to URL-safe slug

```rust
#[sanitizer(SlugSanitizer)]
pub slug: Text,
// "Hello World! This is a Test" → "hello-world-this-is-a-test"
```

**UrlEncodingSanitizer** - URL encode special characters

```rust
#[sanitizer(UrlEncodingSanitizer)]
pub path: Text,
// "hello world" → "hello%20world"
```

### Numeric Sanitizers

**RoundToScaleSanitizer** - Round decimal to specific precision

```rust
#[sanitizer(RoundToScaleSanitizer(2))]
pub price: Decimal,
// 19.999 → 20.00
// 19.994 → 19.99
```

**ClampSanitizer** - Clamp value to range (signed)

```rust
#[sanitizer(ClampSanitizer, min = -100, max = 100)]
pub temperature: Int32,
// 150 → 100
// -150 → -100
```

**ClampUnsignedSanitizer** - Clamp value to range (unsigned)

```rust
#[sanitizer(ClampUnsignedSanitizer, min = 0, max = 100)]
pub percentage: Uint8,
// 150 → 100
// 0 → 0
```

### DateTime Sanitizers

**TimezoneSanitizer** - Convert to specific timezone

```rust
#[sanitizer(TimezoneSanitizer("America/New_York"))]
pub local_time: DateTime,
```

**UtcSanitizer** - Convert to UTC

```rust
#[sanitizer(UtcSanitizer)]
pub timestamp: DateTime,
// Any timezone → UTC
```

### Null Sanitizers

**NullIfEmptySanitizer** - Convert empty strings to null

```rust
#[sanitizer(NullIfEmptySanitizer)]
pub bio: Nullable<Text>,
// "" → Null
// "Hello" → "Hello"
```

---

## Implementing Custom Sanitizers

Create a struct implementing the `Sanitize` trait:

```rust
use wasm_dbms_api::prelude::{Sanitize, Value, DbmsResult};

/// Capitalizes the first letter of each word
pub struct TitleCaseSanitizer;

impl Sanitize for TitleCaseSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Text(text) => {
                let title_case = text
                    .as_str()
                    .split_whitespace()
                    .map(|word| {
                        let mut chars = word.chars();
                        match chars.next() {
                            None => String::new(),
                            Some(first) => {
                                first.to_uppercase().to_string() +
                                chars.as_str().to_lowercase().as_str()
                            }
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                Ok(Value::Text(title_case.into()))
            }
            other => Ok(other),  // Pass through non-text values
        }
    }
}

// Usage
#[sanitizer(TitleCaseSanitizer)]
pub title: Text,
// "hello world" → "Hello World"
```

**Custom sanitizer with parameters:**

```rust
/// Truncates string to max length
pub struct TruncateSanitizer(pub usize);

impl Sanitize for TruncateSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Text(text) => {
                let truncated: String = text.as_str().chars().take(self.0).collect();
                Ok(Value::Text(truncated.into()))
            }
            other => Ok(other),
        }
    }
}

// Usage
#[sanitizer(TruncateSanitizer(100))]
pub summary: Text,
// "very long text..." → truncated to 100 chars
```

**Custom sanitizer with named parameters:**

```rust
/// Replaces a pattern with replacement
pub struct ReplaceSanitizer {
    pub pattern: &'static str,
    pub replacement: &'static str,
}

impl Sanitize for ReplaceSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Text(text) => {
                let replaced = text.as_str().replace(self.pattern, self.replacement);
                Ok(Value::Text(replaced.into()))
            }
            other => Ok(other),
        }
    }
}

// Usage
#[sanitizer(ReplaceSanitizer, pattern = "\n", replacement = " ")]
pub single_line: Text,
```

---

## Sanitization Order

When multiple sanitizers are applied, they run in declaration order:

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    // Order matters!
    #[sanitizer(TrimSanitizer)]              // 1. Trim whitespace
    #[sanitizer(CollapseWhitespaceSanitizer)] // 2. Collapse spaces
    #[sanitizer(LowerCaseSanitizer)]         // 3. Lowercase
    pub email: Text,
}

// Input: "  Alice@Example.COM  "
// After TrimSanitizer: "Alice@Example.COM"
// After CollapseWhitespaceSanitizer: "Alice@Example.COM" (no change)
// After LowerCaseSanitizer: "alice@example.com"
```

**Sanitizers run before validators:**

```rust
#[sanitizer(TrimSanitizer)]           // 1. Trim
#[validate(MaxStrlenValidator(100))]  // 2. Validate length (after trim)
pub name: Text,
```

---

## Examples

**User profile sanitization:**

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    // Clean up name
    #[sanitizer(TrimSanitizer)]
    #[sanitizer(CollapseWhitespaceSanitizer)]
    pub name: Text,

    // Normalize email
    #[sanitizer(TrimSanitizer)]
    #[sanitizer(LowerCaseSanitizer)]
    pub email: Text,

    // Convert empty to null
    #[sanitizer(NullIfEmptySanitizer)]
    pub bio: Nullable<Text>,

    // Uppercase country code
    #[sanitizer(UpperCaseSanitizer)]
    pub country: Nullable<Text>,
}
```

**Financial data sanitization:**

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "transactions"]
pub struct Transaction {
    #[primary_key]
    pub id: Uuid,

    // Round to cents
    #[sanitizer(RoundToScaleSanitizer(2))]
    pub amount: Decimal,

    // Ensure positive (clamp negatives to 0)
    #[sanitizer(ClampUnsignedSanitizer, min = 0, max = 1000000)]
    pub fee: Uint32,

    // Always store in UTC
    #[sanitizer(UtcSanitizer)]
    pub timestamp: DateTime,
}
```

**Content sanitization:**

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "articles"]
pub struct Article {
    #[primary_key]
    pub id: Uuid,

    // Clean title
    #[sanitizer(TrimSanitizer)]
    #[sanitizer(CollapseWhitespaceSanitizer)]
    pub title: Text,

    // Generate URL-safe slug
    #[sanitizer(SlugSanitizer)]
    pub slug: Text,

    // Clean up content
    #[sanitizer(TrimSanitizer)]
    pub content: Text,

    // Optional summary
    #[sanitizer(TrimSanitizer)]
    #[sanitizer(NullIfEmptySanitizer)]
    pub summary: Nullable<Text>,
}
```

**Combined sanitization and validation:**

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "products"]
pub struct Product {
    #[primary_key]
    pub id: Uuid,

    // Sanitize then validate
    #[sanitizer(TrimSanitizer)]
    #[sanitizer(CollapseWhitespaceSanitizer)]
    #[validate(RangeStrlenValidator(1, 200))]
    pub name: Text,

    // Sanitize price to 2 decimals, no validation needed
    #[sanitizer(RoundToScaleSanitizer(2))]
    pub price: Decimal,

    // Create slug and validate format
    #[sanitizer(SlugSanitizer)]
    #[validate(KebabCaseValidator)]
    #[validate(MaxStrlenValidator(100))]
    pub slug: Text,

    // Clean URL and validate format
    #[sanitizer(TrimSanitizer)]
    #[validate(UrlValidator)]
    pub image_url: Nullable<Text>,
}
```
