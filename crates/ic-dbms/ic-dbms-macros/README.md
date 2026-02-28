# ic-dbms-macros

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/ic-dbms-macros.svg)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/ic-dbms-macros.svg)](https://crates.io/crates/ic-dbms-macros)
[![latest-version](https://img.shields.io/crates/v/ic-dbms-macros.svg)](https://crates.io/crates/ic-dbms-macros)
[![ko-fi](https://img.shields.io/badge/donate-ko--fi-red)](https://ko-fi.com/veeso)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/ic-dbms-macros/badge.svg)](https://docs.rs/ic-dbms-macros)

Macros and derive for ic-dbms-canister

This crate provides procedural macros to automatically implement traits
required by the `ic-dbms-canister`.

## Provided Derive Macros

- `Encode`: Automatically implements the `Encode` trait for structs.
- `Table`: Automatically implements the `TableSchema` trait and associated types.
- `DbmsCanister`: Automatically implements the API for the ic-dbms-canister.

## License

This project is licensed under the MIT License. See the [LICENSE](../LICENSE) file for details.
