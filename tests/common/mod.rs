//! Shared test helpers for the configurator integration tests.
//!
//! `RecordingRegistrar` stands in for the production `ClaudeCliRegistrar` so
//! the apply/rollback path can be exercised without spawning the real `claude`
//! binary or touching the user's config.
#![allow(dead_code)] // each test binary uses only part of this module

use std::sync::Mutex;

use kernex_agent::configurator::mcp_registrar::{McpRegistrar, RegisterOutcome};
use kernex_agent::configurator::InstallError;

/// Records `add`/`remove` calls in memory. Optionally fails every `add` (to
/// drive rollback tests).
#[derive(Default)]
pub struct RecordingRegistrar {
    pub adds: Mutex<Vec<(String, String, String)>>, // (name, server_json, scope)
    pub removes: Mutex<Vec<(String, String)>>,      // (name, scope)
    fail_add: bool,
}

impl RecordingRegistrar {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registrar whose `add` always fails, to exercise apply-failure rollback.
    pub fn failing() -> Self {
        Self {
            fail_add: true,
            ..Default::default()
        }
    }

    /// Number of recorded `add` calls.
    pub fn add_count(&self) -> usize {
        self.adds.lock().unwrap().len()
    }

    /// True if an `add` was recorded for `name`.
    pub fn added(&self, name: &str) -> bool {
        self.adds.lock().unwrap().iter().any(|(n, _, _)| n == name)
    }

    /// True if a `remove` was recorded for `name`.
    pub fn removed(&self, name: &str) -> bool {
        self.removes.lock().unwrap().iter().any(|(n, _)| n == name)
    }
}

impl McpRegistrar for RecordingRegistrar {
    fn add(
        &self,
        name: &str,
        server_json: &str,
        scope: &str,
    ) -> Result<RegisterOutcome, InstallError> {
        if self.fail_add {
            return Err(InstallError::Permanent(
                "recording registrar: forced add failure".to_string(),
            ));
        }
        self.adds.lock().unwrap().push((
            name.to_string(),
            server_json.to_string(),
            scope.to_string(),
        ));
        Ok(RegisterOutcome::Registered)
    }

    fn remove(&self, name: &str, scope: &str) -> Result<(), InstallError> {
        self.removes
            .lock()
            .unwrap()
            .push((name.to_string(), scope.to_string()));
        Ok(())
    }
}
