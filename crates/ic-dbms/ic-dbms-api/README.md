# ic-dbms-api

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/ic-dbms-api.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/ic-dbms-api.svg?logo=rust)](https://crates.io/crates/ic-dbms-api)
[![latest-version](https://img.shields.io/crates/v/ic-dbms-api.svg?logo=rust)](https://crates.io/crates/ic-dbms-api)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/ic-dbms-api/badge.svg?logo=rust)](https://docs.rs/ic-dbms-api)

This crate exposes all the types shared between an IC DBMS Canister and an external canister

You can import all the useful types and traits by using the prelude module:

```rust
use ic_dbms_api::prelude::*;
```

## Types

### DBMS

#### Database

- [`Database`](crate::prelude::Database)

#### Foreign Fetcher

- [`ForeignFetcher`](crate::prelude::ForeignFetcher)

#### Init

- [`IcDbmsCanisterArgs`](crate::prelude::IcDbmsCanisterArgs)
- [`IcDbmsCanisterInitArgs`](crate::prelude::IcDbmsCanisterInitArgs)
- [`IcDbmsCanisterUpdateArgs`](crate::prelude::IcDbmsCanisterUpdateArgs)

#### Query

- [`DeleteBehavior`](crate::prelude::DeleteBehavior)
- [`Filter`](crate::prelude::Filter)
- [`JsonCmp`](crate::prelude::JsonCmp)
- [`JsonFilter`](crate::prelude::JsonFilter)
- [`Query`](crate::prelude::Query)
- [`QueryBuilder`](crate::prelude::QueryBuilder)
- [`QueryError`](crate::prelude::QueryError)
- [`QueryResult`](crate::prelude::QueryResult)
- [`OrderDirection`](crate::prelude::OrderDirection)
- [`Select`](crate::prelude::Select)

#### Table

- [`ColumnDef`](crate::prelude::ColumnDef)
- [`ForeignKeyDef`](crate::prelude::ForeignKeyDef)
- [`InsertRecord`](crate::prelude::InsertRecord)
- [`TableColumns`](crate::prelude::TableColumns)
- [`TableError`](crate::prelude::TableError)
- [`TableRecord`](crate::prelude::TableRecord)
- [`UpdateRecord`](crate::prelude::UpdateRecord)
- [`ValuesSource`](crate::prelude::ValuesSource)

### Transaction

- [`TransactionError`](crate::prelude::TransactionError)
- [`TransactionId`](crate::prelude::TransactionId)

#### Dbms Types

- [`Blob`](crate::prelude::Blob)
- [`Boolean`](crate::prelude::Boolean)
- [`Date`](crate::prelude::Date)
- [`DateTime`](crate::prelude::DateTime)
- [`Decimal`](crate::prelude::Decimal)
- [`Int32`](crate::prelude::Int32)
- [`Int64`](crate::prelude::Int64)
- [`Json`](crate::prelude::Json)
- [`Nullable`](crate::prelude::Nullable)
- [`Principal`](crate::prelude::Principal)
- [`Text`](crate::prelude::Text)
- [`Uint32`](crate::prelude::Uint32)
- [`Uint64`](crate::prelude::Uint64)
- [`Uuid`](crate::prelude::Uuid)

#### Sanitizers

- [`Sanitize`](crate::prelude::Sanitize)
- [`ClampSanitizer`](crate::prelude::ClampSanitizer)
- [`ClampUnsignedSanitizer`](crate::prelude::ClampUnsignedSanitizer)
- [`CollapseWhitespaceSanitizer`](crate::prelude::CollapseWhitespaceSanitizer)
- [`LowerCaseSanitizer`](crate::prelude::LowerCaseSanitizer)
- [`NullIfEmptySanitizer`](crate::prelude::NullIfEmptySanitizer)
- [`RoundToScaleSanitizer`](crate::prelude::RoundToScaleSanitizer)
- [`SlugSanitizer`](crate::prelude::SlugSanitizer)
- [`TimezoneSanitizer`](crate::prelude::TimezoneSanitizer)
- [`UtcSanitizer`](crate::prelude::UtcSanitizer)
- [`TrimSanitizer`](crate::prelude::TrimSanitizer)
- [`UpperCaseSanitizer`](crate::prelude::UpperCaseSanitizer)
- [`UrlEncodingSanitizer`](crate::prelude::UrlEncodingSanitizer)

#### Validate

- [`Validate`](crate::prelude::Validate)
- [`CamelCaseValidator`](crate::prelude::CamelCaseValidator)
- [`CountryIso639Validator`](crate::prelude::CountryIso639Validator)
- [`CountryIso3166Validator`](crate::prelude::CountryIso3166Validator)
- [`EmailValidator`](crate::prelude::EmailValidator)
- [`KebabCaseValidator`](crate::prelude::KebabCaseValidator)
- [`MaxStrlenValidator`](crate::prelude::MaxStrlenValidator)
- [`MimeTypeValidator`](crate::prelude::MimeTypeValidator)
- [`MinStrlenValidator`](crate::prelude::MinStrlenValidator)
- [`PhoneNumberValidator`](crate::prelude::PhoneNumberValidator)
- [`RangeStrlenValidator`](crate::prelude::RangeStrlenValidator)
- [`RgbColorValidator`](crate::prelude::RgbColorValidator)
- [`SnakeCaseValidator`](crate::prelude::SnakeCaseValidator)
- [`UrlValidator`](crate::prelude::UrlValidator)

#### Value

- ['DataType'](crate::prelude::DataType)
- [`Value`](crate::prelude::Value)

### Memory

- [`DataSize`](crate::memory::DataSize)
- [`Encode`](crate::memory::Encode)
- [`DecodeError`](crate::memory::DecodeError)
- [`MemoryError`](crate::memory::MemoryError)
- [`MemoryResult`](crate::memory::MemoryResult)
- [`MSize`](crate::memory::MSize)
- [`Page`](crate::memory::Page)
- [`PageOffset`](crate::memory::PageOffset)

## License

This project is licensed under the MIT License. See the [LICENSE](../LICENSE) file for details.
