//! The integration tests of `bussard-download`, linked as one binary.
//!
//! Each module is a former `tests/<module>.rs` file. One binary instead of
//! one per file saves a link against the full dependency graph per file;
//! nextest still runs every test in its own process.

mod apply_mock;
mod compare_rel_mem;
mod enum_default_plan;
mod ets_golden;
mod flash_corpus;
mod flash_dryrun_corpus;
mod flash_dryrun_system7;
mod flash_mock;
mod flash_mock_system7;
mod golden_images;
mod golden_tables;
mod param_readback_roundtrip;
mod sweep_fixtures;
mod sys7_jung_m2_plan;
mod sys7_theben_0701_plan;
