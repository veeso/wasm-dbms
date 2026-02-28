//! This module exposes all the integrity validators for the DBMS.

mod common;
mod insert;
mod update;

pub use self::insert::InsertIntegrityValidator;
pub use self::update::UpdateIntegrityValidator;
