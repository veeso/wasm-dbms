// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! Permission types backing the granular ACL.
//!
//! These types are reused by the storage layer (`wasm-dbms-memory::acl`),
//! the engine (`wasm-dbms::Dbms`) and the IC canister surface. They live
//! in `wasm-dbms-api` so that [`crate::error::DbmsError::AccessDenied`]
//! can reference them without pulling in the memory crate.

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

use crate::dbms::table::TableFingerprint;

bitflags! {
    /// Per-table permission bits.
    ///
    /// Bits combine via the `BitOr`/`BitAnd` operators provided by
    /// [`bitflags`]. The wire encoding is a single `u8`, mirrored on the
    /// IC canister surface as a Candid `nat8`.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct TablePerms: u8 {
        /// Permission to run `SELECT` / `aggregate` against the table.
        const READ   = 0b0001;
        /// Permission to run `INSERT` against the table.
        const INSERT = 0b0010;
        /// Permission to run `UPDATE` against the table.
        const UPDATE = 0b0100;
        /// Permission to run `DELETE` against the table.
        const DELETE = 0b1000;
    }
}

#[cfg(feature = "candid")]
impl candid::CandidType for TablePerms {
    fn _ty() -> candid::types::Type {
        candid::types::TypeInner::Nat8.into()
    }

    fn idl_serialize<S>(&self, serializer: S) -> Result<(), S::Error>
    where
        S: candid::types::Serializer,
    {
        serializer.serialize_nat8(self.bits())
    }
}

/// Marker passed in [`crate::error::DbmsError::AccessDenied`] to describe
/// the perm that was missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum RequiredPerm {
    /// A specific table permission set was required.
    Table(TablePerms),
    /// The `admin` bypass flag was required.
    Admin,
    /// The `manage_acl` operational flag was required.
    ManageAcl,
    /// The `migrate` operational flag was required.
    Migrate,
}

/// Effective permission set carried by a single identity.
///
/// `per_table` is encoded as a `Vec<(TableFingerprint, TablePerms)>` so
/// the type maps cleanly onto Candid (no `HashMap` on the wire). Lookup
/// happens via linear scan; the table count is bounded by the schema.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct IdentityPerms {
    /// Bypass all table checks. Does NOT imply `manage_acl` or `migrate`.
    pub admin: bool,
    /// Permission to grant/revoke perms and add/remove identities.
    pub manage_acl: bool,
    /// Permission to run `Dbms::migrate()`.
    pub migrate: bool,
    /// Table perms applied to every table. Unioned with `per_table`.
    pub all_tables: TablePerms,
    /// Per-table perms. Additive over `all_tables`.
    pub per_table: Vec<(TableFingerprint, TablePerms)>,
}

impl IdentityPerms {
    /// Fully-permissive perms, used by `NoAccessControl`.
    pub fn fully_permissive() -> Self {
        Self {
            admin: true,
            manage_acl: true,
            migrate: true,
            all_tables: TablePerms::all(),
            per_table: Vec::new(),
        }
    }

    /// Returns whether this identity is granted `required` on `table`.
    pub fn grants_table(&self, table: TableFingerprint, required: TablePerms) -> bool {
        if self.admin {
            return true;
        }
        let union = self.all_tables | self.lookup(table);
        union.contains(required)
    }

    /// Returns the perms entry for `table`, or empty if not present.
    fn lookup(&self, table: TableFingerprint) -> TablePerms {
        self.per_table
            .iter()
            .find(|(t, _)| *t == table)
            .map(|(_, p)| *p)
            .unwrap_or_default()
    }

    /// Applies `grant` in place. Idempotent.
    pub fn apply_grant(&mut self, grant: PermGrant) {
        match grant {
            PermGrant::Admin => self.admin = true,
            PermGrant::ManageAcl => self.manage_acl = true,
            PermGrant::Migrate => self.migrate = true,
            PermGrant::AllTables(p) => self.all_tables |= p,
            PermGrant::Table(t, p) => match self.per_table.iter_mut().find(|(tt, _)| *tt == t) {
                Some((_, existing)) => *existing |= p,
                None => self.per_table.push((t, p)),
            },
        }
    }

    /// Applies `revoke` in place. Idempotent. Removes empty per-table entries.
    pub fn apply_revoke(&mut self, revoke: PermRevoke) {
        match revoke {
            PermRevoke::Admin => self.admin = false,
            PermRevoke::ManageAcl => self.manage_acl = false,
            PermRevoke::Migrate => self.migrate = false,
            PermRevoke::AllTables(p) => self.all_tables.remove(p),
            PermRevoke::Table(t, p) => {
                if let Some(pos) = self.per_table.iter().position(|(tt, _)| *tt == t) {
                    self.per_table[pos].1.remove(p);
                    if self.per_table[pos].1.is_empty() {
                        self.per_table.swap_remove(pos);
                    }
                }
            }
        }
    }

    /// True when the identity carries no perms whatsoever.
    pub fn is_empty(&self) -> bool {
        !self.admin
            && !self.manage_acl
            && !self.migrate
            && self.all_tables.is_empty()
            && self.per_table.is_empty()
    }
}

/// Grant action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum PermGrant {
    /// Grant the `admin` bypass flag.
    Admin,
    /// Grant the `manage_acl` operational flag.
    ManageAcl,
    /// Grant the `migrate` operational flag.
    Migrate,
    /// Grant the given perm bits on every table.
    AllTables(TablePerms),
    /// Grant the given perm bits on a specific table.
    Table(TableFingerprint, TablePerms),
}

/// Revoke action — symmetric to [`PermGrant`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum PermRevoke {
    /// Revoke the `admin` bypass flag.
    Admin,
    /// Revoke the `manage_acl` operational flag.
    ManageAcl,
    /// Revoke the `migrate` operational flag.
    Migrate,
    /// Revoke the given perm bits from every table (does not affect
    /// per-table grants).
    AllTables(TablePerms),
    /// Revoke the given perm bits from a specific table.
    Table(TableFingerprint, TablePerms),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dbms::table::fingerprint_for_name;

    fn fp(name: &str) -> TableFingerprint {
        fingerprint_for_name(name)
    }

    #[test]
    fn test_admin_bypasses_table_check() {
        let mut p = IdentityPerms::default();
        p.admin = true;
        assert!(p.grants_table(fp("users"), TablePerms::DELETE));
    }

    #[test]
    fn test_all_tables_grant_unions_with_per_table() {
        let mut p = IdentityPerms::default();
        p.all_tables = TablePerms::READ;
        p.apply_grant(PermGrant::Table(
            fp("users"),
            TablePerms::INSERT | TablePerms::UPDATE,
        ));
        assert!(p.grants_table(fp("users"), TablePerms::READ));
        assert!(p.grants_table(fp("users"), TablePerms::INSERT));
        assert!(!p.grants_table(fp("users"), TablePerms::DELETE));
        assert!(p.grants_table(fp("posts"), TablePerms::READ));
        assert!(!p.grants_table(fp("posts"), TablePerms::INSERT));
    }

    #[test]
    fn test_apply_grant_is_idempotent() {
        let mut p = IdentityPerms::default();
        p.apply_grant(PermGrant::Admin);
        p.apply_grant(PermGrant::Admin);
        assert!(p.admin);
        p.apply_grant(PermGrant::Table(fp("users"), TablePerms::READ));
        p.apply_grant(PermGrant::Table(fp("users"), TablePerms::READ));
        assert_eq!(p.per_table.len(), 1);
        assert_eq!(p.per_table[0], (fp("users"), TablePerms::READ));
    }

    #[test]
    fn test_revoke_partial_table_bits() {
        let mut p = IdentityPerms::default();
        p.apply_grant(PermGrant::Table(
            fp("users"),
            TablePerms::READ | TablePerms::INSERT | TablePerms::DELETE,
        ));
        p.apply_revoke(PermRevoke::Table(
            fp("users"),
            TablePerms::INSERT | TablePerms::DELETE,
        ));
        assert_eq!(p.per_table[0].1, TablePerms::READ);
    }

    #[test]
    fn test_revoke_all_table_bits_removes_entry() {
        let mut p = IdentityPerms::default();
        p.apply_grant(PermGrant::Table(fp("users"), TablePerms::READ));
        p.apply_revoke(PermRevoke::Table(fp("users"), TablePerms::READ));
        assert!(p.per_table.is_empty());
    }

    #[test]
    fn test_admin_does_not_imply_manage_acl_or_migrate() {
        let mut p = IdentityPerms::default();
        p.admin = true;
        assert!(!p.manage_acl);
        assert!(!p.migrate);
    }

    #[test]
    fn test_fully_permissive_grants_everything() {
        let p = IdentityPerms::fully_permissive();
        assert!(p.admin && p.manage_acl && p.migrate);
        assert!(p.grants_table(fp("anything"), TablePerms::all()));
    }

    #[test]
    fn test_is_empty_after_revoking_all() {
        let mut p = IdentityPerms::default();
        p.apply_grant(PermGrant::Admin);
        assert!(!p.is_empty());
        p.apply_revoke(PermRevoke::Admin);
        assert!(p.is_empty());
    }
}
