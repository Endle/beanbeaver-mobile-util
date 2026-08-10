//! Local `uniffi-bindgen` entry point (library mode), used by each app's build
//! script to emit the language glue from the compiled core:
//!
//!   # beanbeaver-ios/build-xcframework.sh
//!   cargo run -p beanbeaver-ios-ffi-build --bin uniffi-bindgen -- \
//!     generate --library target/debug/libbb_mobile_ffi.dylib \
//!     --language swift --out-dir <dir>
//!
//!   # beanbeaver-android/build-android.sh
//!   cargo run -p beanbeaver-android-ffi-build --bin uniffi-bindgen -- \
//!     generate --library target/debug/libbb_mobile_ffi.so \
//!     --language kotlin --out-dir <dir>
//!
//! `--library` mode scans the whole artifact, and `libbb_mobile_ffi` carries
//! two crates' scaffolding, so **one run emits both namespaces** —
//! `uniffi/bb_mobile_ffi/…` and `uniffi/bb_receipt_ffi/…`.
//!
//! Both crates ship this same bin, but they are git dependencies (not workspace
//! members), so `cargo run -p bb-mobile-ffi --bin uniffi-bindgen` can't reach
//! their copies ("not found in workspace"). Each app hosts its own via a
//! `[[bin]]` pointing here, which also keeps `uniffi_bindgen_main()` compiled
//! against the app's own `uniffi` dependency — that version must stay in step
//! with the one the FFI crates pull in (0.28).
fn main() {
    uniffi::uniffi_bindgen_main()
}
