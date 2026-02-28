use candid::CandidType;
use serde::{Deserialize, Serialize};

/// Arguments for initializing or updating an IC DBMS canister.
#[derive(Debug, CandidType, Serialize, Deserialize)]
pub enum IcDbmsCanisterArgs {
    Init(IcDbmsCanisterInitArgs),
    Upgrade(IcDbmsCanisterUpgradeArgs),
}

impl IcDbmsCanisterArgs {
    /// Unwraps the arguments as [`IcDbmsCanisterInitArgs`], or traps if it's not of that variant.
    pub fn unwrap_init(self) -> IcDbmsCanisterInitArgs {
        match self {
            IcDbmsCanisterArgs::Init(args) => args,
            _ => ic_cdk::trap("Expected IcDbmsCanisterArgs::Init"),
        }
    }

    /// Unwraps the arguments as [`IcDbmsCanisterUpgradeArgs`], or traps if it's not of that variant.
    pub fn unwrap_update(self) -> IcDbmsCanisterUpgradeArgs {
        match self {
            IcDbmsCanisterArgs::Upgrade(args) => args,
            _ => ic_cdk::trap("Expected IcDbmsCanisterArgs::Upgrade"),
        }
    }
}

#[derive(Debug, CandidType, Serialize, Deserialize)]
pub struct IcDbmsCanisterInitArgs {
    pub allowed_principals: Vec<candid::Principal>,
}

#[derive(Debug, CandidType, Serialize, Deserialize)]
pub struct IcDbmsCanisterUpgradeArgs;
