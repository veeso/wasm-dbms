/// Result type for IC DBMS Canister client operations.
pub type IcDbmsCanisterClientResult<T> = Result<T, IcDbmCanisterClientError>;

/// Errors that can occur when interacting with an IC DBMS Canister.
#[derive(thiserror::Error, Debug)]
pub enum IcDbmCanisterClientError {
    #[error("IC Call failed: {0}")]
    Call(#[from] ic_cdk::call::CallFailed),
    #[error("Candid decode failed: {0}")]
    Candid(#[from] ic_cdk::call::CandidDecodeFailed),
    #[error("IC DBMS Canister error: {0}")]
    Canister(#[from] ic_dbms_api::prelude::IcDbmsError),
    #[error("IC Agent error: {0}")]
    #[cfg(feature = "ic-agent")]
    IcAgent(#[from] IcAgentError),
    #[error("Pocket IC error: {0}")]
    #[cfg(feature = "pocket-ic")]
    PocketIc(#[from] PocketIcError),
}

/// Errors that can occur when using the ic-agent client.
#[cfg(feature = "ic-agent")]
#[cfg_attr(docsrs, doc(cfg(feature = "ic-agent")))]
#[derive(thiserror::Error, Debug)]
pub enum IcAgentError {
    #[error(transparent)]
    Agent(#[from] ic_agent::AgentError),
    #[error("Candid error: {0}")]
    Candid(#[from] candid::Error),
}

/// Errors that can occur when using the pocket-ic client.
#[cfg(feature = "pocket-ic")]
#[cfg_attr(docsrs, doc(cfg(feature = "pocket-ic")))]
#[derive(thiserror::Error, Debug)]
pub enum PocketIcError {
    #[error("Pocket IC call failed: {0}")]
    Candid(#[from] candid::Error),
    #[error("Pocket IC reject response: {0}")]
    Reject(pocket_ic::RejectResponse),
}

#[cfg(feature = "pocket-ic")]
#[cfg_attr(docsrs, doc(cfg(feature = "pocket-ic")))]
impl From<pocket_ic::RejectResponse> for PocketIcError {
    fn from(reject: pocket_ic::RejectResponse) -> Self {
        PocketIcError::Reject(reject)
    }
}
