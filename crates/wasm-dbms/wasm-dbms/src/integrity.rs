// Rust guideline compliant 2026-02-28

//! Integrity validators for insert and update operations.

pub(crate) mod common;
mod insert;
mod update;

pub use self::insert::InsertIntegrityValidator;
pub use self::update::UpdateIntegrityValidator;
