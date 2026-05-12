//! Shared harness for the PTY-driven UI tests.  Each `tests/ui_*.rs` test
//! binary opts in via `mod ui_common;`.

#![allow(dead_code)]

pub mod fixtures;
pub mod harness;
pub mod keys;

#[allow(unused_imports)]
pub use fixtures::Fixture;
#[allow(unused_imports)]
pub use harness::{SessionOptions, TxtSession};
#[allow(unused_imports)]
pub use keys::{Arrow, Key};
