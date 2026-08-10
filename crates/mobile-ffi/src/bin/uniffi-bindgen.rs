//! Bindings generator for this workspace, so `mobile-ffi` can be exercised
//! without an app checked out:
//!
//!   cargo build
//!   cargo run --bin uniffi-bindgen -- generate \
//!     --library target/debug/libbb_mobile_ffi.dylib \
//!     --language kotlin --out-dir /tmp/gen
//!
//! Each app hosts its own identical copy at `shared/src/bin/uniffi-bindgen.rs`,
//! compiled into that app's package so it matches that app's `uniffi` pin.
fn main() {
    uniffi::uniffi_bindgen_main()
}
