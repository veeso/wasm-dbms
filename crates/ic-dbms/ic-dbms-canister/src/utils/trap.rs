/// A utility module for canister trapping.
///
/// This module provides a function and a macro to facilitate trapping the canister execution.
/// On WebAssembly targets, it uses `ic_cdk::trap`, while on non-Wasm targets, it uses Rust's standard panic mechanism.
/// On non-Wasm targets, it just `panic!`s with the provided message.
pub fn trap(msg: String) -> ! {
    #[cfg(target_family = "wasm")]
    {
        ic_cdk::trap(msg);
    }
    #[cfg(not(target_family = "wasm"))]
    {
        std::panic::panic_any(msg)
    }
}

/// A macro for trapping the canister execution.
///
/// # Examples
///
/// ```rust,no_run
/// use ic_dbms_canister::trap;
/// let name = "IC DBMS Canister";
/// trap!("Hello, {name}!");
/// trap!("Canister trapped.");
/// trap!("This is a debug message with a value: {}", 42);
/// ```
#[macro_export]
macro_rules! trap {
    ($($key:tt $(:$capture:tt)? $(= $value:expr)?),+; $($arg:tt)+) => ({
        $crate::utils::trap(format!($($arg)+));
    });

    ( $($arg:tt)+) => ({
        $crate::utils::trap(format!($($arg)+));
    });
}

#[cfg(test)]
mod tests {

    #[test]
    #[should_panic(expected = "This is a test trap message with value: 100")]
    fn test_trap_macro() {
        crate::trap!("This is a test trap message with value: {}", 100);
    }
}
