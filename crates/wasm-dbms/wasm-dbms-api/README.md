# wasm-dbms-api

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/wasm-dbms-api.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/wasm-dbms-api.svg?logo=rust)](https://crates.io/crates/wasm-dbms-api)
[![latest-version](https://img.shields.io/crates/v/wasm-dbms-api.svg?logo=rust)](https://crates.io/crates/wasm-dbms-api)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/wasm-dbms-api/badge.svg?logo=rust)](https://docs.rs/wasm-dbms-api)

Runtime-agnostic API types and traits for the wasm-dbms DBMS engine.

This crate provides all shared types, traits, and abstractions needed to interact with
a wasm-dbms instance. It is independent of any specific WASM runtime (IC, WASI, Wasmtime, etc.).

Import all useful types and traits via the prelude:

```rust
use wasm_dbms_api::prelude::*;
```

## Feature Flags

- `candid`: Enables `CandidType` derives on all public types and exposes Candid-specific API boundary types.

## Types

### DBMS

#### Database

- [`Database`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.Database.html)

#### Query

- [`DeleteBehavior`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.DeleteBehavior.html)
- [`Filter`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Filter.html)
- [`Join`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Join.html)
- [`JoinType`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.JoinType.html)
- [`JsonCmp`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.JsonCmp.html)
- [`JsonFilter`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.JsonFilter.html)
- [`Query`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Query.html)
- [`QueryBuilder`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.QueryBuilder.html)
- [`QueryError`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.QueryError.html)
- [`QueryResult`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.QueryResult.html)
- [`OrderDirection`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.OrderDirection.html)
- [`Select`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.Select.html)

#### Table

- [`Autoincrement`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.Autoincrement.html)
- [`ColumnDef`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.ColumnDef.html)
- [`ForeignKeyDef`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.ForeignKeyDef.html)
- [`InsertRecord`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.InsertRecord.html)
- [`TableColumns`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.TableColumns.html)
- [`TableError`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.TableError.html)
- [`TableRecord`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.TableRecord.html)
- [`TableSchema`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.TableSchema.html)
- [`UpdateRecord`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.UpdateRecord.html)
- [`ValuesSource`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.ValuesSource.html)

#### Foreign Fetcher

- [`ForeignFetcher`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.ForeignFetcher.html)
- [`NoForeignFetcher`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.NoForeignFetcher.html)

#### Transaction

- [`TransactionError`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.TransactionError.html)
- [`TransactionId`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/type.TransactionId.html)

### Data Types

- [`Blob`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Blob.html)
- [`Boolean`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Boolean.html)
- [`Date`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Date.html)
- [`DateTime`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.DateTime.html)
- [`Decimal`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Decimal.html)
- [`Int8`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Int8.html)
- [`Int16`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Int16.html)
- [`Int32`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Int32.html)
- [`Int64`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Int64.html)
- [`Json`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Json.html)
- [`Nullable`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.Nullable.html)
- [`Text`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Text.html)
- [`Uint8`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Uint8.html)
- [`Uint16`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Uint16.html)
- [`Uint32`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Uint32.html)
- [`Uint64`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Uint64.html)
- [`Uuid`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.Uuid.html)
- [`CustomValue`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.CustomValue.html)
- [`Value`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.Value.html)

### Sanitizers

- [`Sanitize`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.Sanitize.html)
- [`ClampSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.ClampSanitizer.html)
- [`ClampUnsignedSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.ClampUnsignedSanitizer.html)
- [`CollapseWhitespaceSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.CollapseWhitespaceSanitizer.html)
- [`LowerCaseSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.LowerCaseSanitizer.html)
- [`NullIfEmptySanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.NullIfEmptySanitizer.html)
- [`RoundToScaleSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.RoundToScaleSanitizer.html)
- [`SlugSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.SlugSanitizer.html)
- [`TimezoneSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.TimezoneSanitizer.html)
- [`UtcSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.UtcSanitizer.html)
- [`TrimSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.TrimSanitizer.html)
- [`UpperCaseSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.UpperCaseSanitizer.html)
- [`UrlEncodingSanitizer`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.UrlEncodingSanitizer.html)

### Validators

- [`Validate`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.Validate.html)
- [`CamelCaseValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.CamelCaseValidator.html)
- [`CountryIso639Validator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.CountryIso639Validator.html)
- [`CountryIso3166Validator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.CountryIso3166Validator.html)
- [`EmailValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.EmailValidator.html)
- [`KebabCaseValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.KebabCaseValidator.html)
- [`MaxStrlenValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.MaxStrlenValidator.html)
- [`MimeTypeValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.MimeTypeValidator.html)
- [`MinStrlenValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.MinStrlenValidator.html)
- [`PhoneNumberValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.PhoneNumberValidator.html)
- [`RangeStrlenValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.RangeStrlenValidator.html)
- [`RgbColorValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.RgbColorValidator.html)
- [`SnakeCaseValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.SnakeCaseValidator.html)
- [`UrlValidator`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/struct.UrlValidator.html)

### Memory

- [`DataSize`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.DataSize.html)
- [`Encode`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/trait.Encode.html)
- [`DecodeError`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.DecodeError.html)
- [`MemoryError`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.MemoryError.html)
- [`MemoryResult`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/type.MemoryResult.html)
- [`MSize`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/type.MSize.html)
- [`Page`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/type.Page.html)
- [`PageOffset`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/type.PageOffset.html)

### Error

- [`DbmsError`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/enum.DbmsError.html)
- [`DbmsResult`](https://docs.rs/wasm-dbms-api/latest/wasm_dbms_api/prelude/type.DbmsResult.html)

## License

This project is licensed under the MIT License. See the [LICENSE](../../../LICENSE) file for details.
