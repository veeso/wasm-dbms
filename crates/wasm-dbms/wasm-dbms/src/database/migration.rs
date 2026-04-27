//! Schema migration engine.
//!
//! Hosts the diff/plan/apply pipeline that reconciles the compile-time
//! [`DatabaseSchema`](crate::schema::DatabaseSchema) with the
//! [`TableSchemaSnapshot`](wasm_dbms_api::prelude::TableSchemaSnapshot) records
//! persisted in stable memory. Drift detection is the entry point: every CRUD
//! call on [`WasmDbmsDatabase`](crate::database::WasmDbmsDatabase) consults the
//! cached drift flag and refuses to proceed while the schemas disagree.
//!
//! See `.claude/plans/2026-04-27-schema-migrations-engine-plan.md` for the full
//! design context.

pub(crate) mod apply;
pub(crate) mod diff;
pub(crate) mod plan;
pub(crate) mod snapshots;
