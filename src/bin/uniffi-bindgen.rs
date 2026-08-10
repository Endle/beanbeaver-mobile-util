//! Local `uniffi-bindgen` entry point (library mode), used by each app's build
//! script to emit the language glue from the compiled core:
//!
//!   # beanbeaver-ios/build-xcframework.sh
//!   cargo run -p beanbeaver-ios-ffi-build --bin uniffi-bindgen -- \
//!     generate --library target/debug/libbb_receipt_ffi.dylib \
//!     --language swift --out-dir <dir>
//!
//!   # beanbeaver-android/build-android.sh
//!   cargo run -p beanbeaver-android-ffi-build --bin uniffi-bindgen -- \
//!     generate --library target/debug/libbb_receipt_ffi.so \
//!     --language kotlin --out-dir <dir>
//!
//! bb-receipt-ffi ships this same bin, but it's a git dependency (not a
//! workspace member), so `cargo run -p bb-receipt-ffi --bin uniffi-bindgen`
//! can't reach it ("not found in workspace"). Each app hosts its own via a
//! `[[bin]]` pointing here, which also keeps `uniffi_bindgen_main()` compiled
//! against the app's own `uniffi` dependency — that version must stay in step
//! with the one `bb-receipt-ffi` pulls in (0.28).
fn main() {
    uniffi::uniffi_bindgen_main()
}
