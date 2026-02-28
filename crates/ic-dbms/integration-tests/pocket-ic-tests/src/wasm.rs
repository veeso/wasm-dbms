use std::path::Path;

pub enum Canister {
    DbmsCanister,
    DbmsCanisterClientIntegration,
}

impl Canister {
    pub fn as_path(&self) -> &'static Path {
        match self {
            Canister::DbmsCanister => Path::new("../../../../.artifact/example.wasm.gz"),
            Canister::DbmsCanisterClientIntegration => {
                Path::new("../../../../.artifact/dbms_canister_client_integration.wasm.gz")
            }
        }
    }
}
