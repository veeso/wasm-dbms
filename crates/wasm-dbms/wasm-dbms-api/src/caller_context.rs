/// Provides caller identity to the DBMS layer.
///
/// Implementations of this trait supply an opaque caller identifier and
/// ownership status. The DBMS engine uses this information for ACL checks
/// without depending on any specific identity scheme.
///
/// # IC example
///
/// ```ignore
/// struct IcCallerContext(candid::Principal);
///
/// impl CallerContext for IcCallerContext {
///     fn caller(&self) -> Vec<u8> {
///         self.0.as_slice().to_vec()
///     }
///
///     fn is_owner(&self) -> bool {
///         self.0 == ic_cdk::api::id()
///     }
/// }
/// ```
pub trait CallerContext {
    /// Returns an opaque byte representation of the caller's identity.
    fn caller(&self) -> Vec<u8>;

    /// Returns whether the caller is the module owner (admin).
    fn is_owner(&self) -> bool;
}
