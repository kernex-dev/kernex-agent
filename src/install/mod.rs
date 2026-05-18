//! Install pipeline submodules: audit writer, preset resolver, CLI wiring.
//!
//! Sibling to `src/configurator/` (the 7-stage pipeline). The configurator
//! owns the stage orchestration; this module owns the install-time
//! support surfaces (audit log, preset table, CLI flags).

pub mod audit;
pub mod preset;
