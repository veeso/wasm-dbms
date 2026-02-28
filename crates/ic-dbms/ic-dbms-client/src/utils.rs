/// Utility functions for IC DBMS Client.
///
/// Given a table name and a method name, returns the combined method name
/// used to call the corresponding method on the IC DBMS Canister.
///
/// That's because the API methods for table operations are named using the pattern
/// `<method>_<table>`, e.g., `insert_users`, `update_orders`, etc.
#[inline]
pub fn table_method(table: &str, method: &str) -> String {
    format!("{method}_{table}")
}
