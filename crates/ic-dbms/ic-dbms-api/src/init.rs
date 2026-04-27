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
    /// Initial admins to bootstrap the granular ACL with full perms
    /// (`admin` + `manage_acl` + `migrate` + every table perm). When
    /// `None` or empty, the deployer principal is used.
    pub allowed_principals: Option<Vec<candid::Principal>>,
}

#[derive(Debug, CandidType, Serialize, Deserialize)]
pub struct IcDbmsCanisterUpgradeArgs;

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_unwrap_init_on_init_variant() {
        let principals = vec![candid::Principal::anonymous()];
        let args = IcDbmsCanisterArgs::Init(IcDbmsCanisterInitArgs {
            allowed_principals: Some(principals.clone()),
        });
        let init = args.unwrap_init();
        assert_eq!(init.allowed_principals, Some(principals));
    }

    #[test]
    fn test_unwrap_update_on_upgrade_variant() {
        let args = IcDbmsCanisterArgs::Upgrade(IcDbmsCanisterUpgradeArgs);
        let _upgrade = args.unwrap_update();
    }

    #[test]
    #[should_panic]
    fn test_unwrap_init_on_upgrade_variant_traps() {
        let args = IcDbmsCanisterArgs::Upgrade(IcDbmsCanisterUpgradeArgs);
        let _init = args.unwrap_init();
    }

    #[test]
    #[should_panic]
    fn test_unwrap_update_on_init_variant_traps() {
        let args = IcDbmsCanisterArgs::Init(IcDbmsCanisterInitArgs {
            allowed_principals: Some(vec![]),
        });
        let _upgrade = args.unwrap_update();
    }

    #[test]
    fn test_candid_roundtrip_init_args() {
        let args = IcDbmsCanisterArgs::Init(IcDbmsCanisterInitArgs {
            allowed_principals: Some(vec![candid::Principal::anonymous()]),
        });
        let encoded = candid::encode_one(&args).expect("failed to encode");
        let decoded: IcDbmsCanisterArgs = candid::decode_one(&encoded).expect("failed to decode");
        assert!(matches!(decoded, IcDbmsCanisterArgs::Init(_)));
    }

    #[test]
    fn test_init_args_default_allowed_is_none() {
        let args = IcDbmsCanisterArgs::Init(IcDbmsCanisterInitArgs {
            allowed_principals: None,
        });
        let init = args.unwrap_init();
        assert!(init.allowed_principals.is_none());
    }

    #[test]
    fn test_candid_roundtrip_upgrade_args() {
        let args = IcDbmsCanisterArgs::Upgrade(IcDbmsCanisterUpgradeArgs);
        let encoded = candid::encode_one(&args).expect("failed to encode");
        let decoded: IcDbmsCanisterArgs = candid::decode_one(&encoded).expect("failed to decode");
        assert!(matches!(decoded, IcDbmsCanisterArgs::Upgrade(_)));
    }
}
