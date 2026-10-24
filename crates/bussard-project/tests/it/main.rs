//! The integration tests of `bussard-project`, linked as one binary.
//!
//! Each module is a former `tests/<module>.rs` file. One binary instead of
//! one per file saves a link against the full dependency graph per file;
//! nextest still runs every test in its own process.

mod default_params;
mod ets_versions;
mod import;
mod import_adversarial;
mod lock_keys;
