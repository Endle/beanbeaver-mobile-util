//! Personal price history over a snapshot of receipt observations.
//!
//! [`build_history`] groups exact merchant/name/code tuples unless the caller
//! supplies explicit [`ProductLink`]s. Neither equal codes across merchants nor
//! a missing code imply identity. Removing a link splits the next snapshot.
//!
//! Receipt amounts always survive. Comparable prices require an explicit
//! [`ComparisonApproval`]: the parser's default quantity of one is not evidence.
//! See the crate README for the app projection and persistence contract.

#![doc = include_str!("../README.md")]

mod history;
mod types;

pub use history::{build_history, search};
pub use types::*;
