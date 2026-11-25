//! Which tools a `kx serve` job may use.
//!
//! A serve job runs with nobody watching, on text the operator did not write:
//! a webhook payload, a file the job was asked to review, a page it fetched.
//! Anything in that text can try to steer the model. With every tool
//! available, a successful attempt can run commands, rewrite files and send
//! data out. With read-only tools the worst outcome is a wrong answer in the
//! job output, which only the caller sees.
//!
//! The profile decides which tools a job may call; the runtime decides
//! where they reach. The in-process file tools refuse every path outside
//! the server's working directory, `--read-path` directories and the
//! project's skills, and never open the memory database or the user's
//! config, whatever the profile.

use std::path::Path;

use async_trait::async_trait;
use kernex_core::hooks::{HookOutcome, HookRunner};
use serde_json::Value;

use crate::agent_sandbox::{AccessArgs, ToolAccess};
use crate::runtime_glue::ProviderSetup;

/// Tool profile for every job of one `kx serve` process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ToolProfile {
    /// Read, search and list files. No shell, no writes, no network.
    #[default]
    ReadOnly,
    /// Every tool, including the shell, file writes, `web_fetch`, MCP
    /// servers and toolboxes.
    Full,
}

/// In-process tools a read-only job may call (API providers).
const READ_ONLY_TOOLS: &[&str] = &["read", "grep", "glob"];

/// The same set under the names Claude Code uses for its own tools.
const READ_ONLY_CLAUDE_CODE_TOOLS: &[&str] = &["Read", "Grep", "Glob"];

impl ToolProfile {
    /// Whether a job under this profile may call `tool`.
    pub fn allows(self, tool: &str) -> bool {
        match self {
            ToolProfile::Full => true,
            ToolProfile::ReadOnly => READ_ONLY_TOOLS
                .iter()
                .any(|allowed| tool.eq_ignore_ascii_case(allowed)),
        }
    }

    /// Tool allowlist for providers that run their own tools in a child
    /// process, where the hook below never sees a call. `None` means no
    /// restriction.
    pub fn cli_tool_allowlist(self) -> Option<Vec<String>> {
        match self {
            ToolProfile::Full => None,
            ToolProfile::ReadOnly => Some(
                READ_ONLY_CLAUDE_CODE_TOOLS
                    .iter()
                    .map(|t| (*t).to_string())
                    .collect(),
            ),
        }
    }

    /// Name as the operator types it.
    pub fn as_str(self) -> &'static str {
        match self {
            ToolProfile::ReadOnly => "read-only",
            ToolProfile::Full => "full",
        }
    }
}

/// What every job of one `kx serve` process may use: the tool profile, and
/// where `web_fetch` and the file tools may reach (`--web-fetch`,
/// `--read-path` and `~/.kx/config.toml`; serve reads no `.kx.toml`).
#[derive(Clone, Debug, Default)]
pub struct JobTools {
    pub profile: ToolProfile,
    pub access: ToolAccess,
}

impl JobTools {
    /// Resolve the access for `profile`. A read-only job has no network,
    /// so `web_fetch` stays off under it whatever the flags say.
    pub fn new(profile: ToolProfile, args: &AccessArgs) -> anyhow::Result<Self> {
        Ok(Self::with_access(
            profile,
            ToolAccess::without_project(args)?,
        ))
    }

    fn with_access(profile: ToolProfile, mut access: ToolAccess) -> Self {
        if profile == ToolProfile::ReadOnly && access.web_fetch.is_enabled() {
            tracing::warn!(
                "web_fetch stays off under the read-only tool profile; start the server \
                 with `--tools full` to use it"
            );
            access.web_fetch = Default::default();
        }
        Self { profile, access }
    }

    /// The provider setup for a job that keeps its state in `data_dir`.
    pub fn provider_setup(&self, data_dir: &Path) -> ProviderSetup {
        ProviderSetup {
            data_dir: data_dir.to_path_buf(),
            access: self.access.clone(),
            cli_tools: self.profile.cli_tool_allowlist(),
        }
    }
}

/// Pre-tool hook that enforces a [`ToolProfile`] on in-process tool calls.
pub struct ServeHookRunner {
    pub profile: ToolProfile,
    pub verbose: bool,
}

#[async_trait]
impl HookRunner for ServeHookRunner {
    async fn pre_tool(&self, tool_name: &str, _input: &Value) -> HookOutcome {
        if !self.profile.allows(tool_name) {
            tracing::warn!(
                tool = tool_name,
                "tool call refused by the serve tool profile"
            );
            return HookOutcome::Blocked(format!(
                "`{tool_name}` is not available: this server runs jobs with the \
                 `{}` tool profile. The operator can start it with `--tools full`.",
                self.profile.as_str()
            ));
        }
        if self.verbose {
            eprintln!("[tool] {tool_name}");
        }
        HookOutcome::Allow
    }

    async fn post_tool(&self, tool_name: &str, _result: &str, is_error: bool) {
        if self.verbose && is_error {
            eprintln!("[tool error] {tool_name}");
        }
    }

    async fn on_stop(&self, _final_text: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_read_only() {
        assert_eq!(ToolProfile::default(), ToolProfile::ReadOnly);
    }

    #[test]
    fn read_only_allows_reading_and_nothing_else() {
        let p = ToolProfile::ReadOnly;
        for tool in ["read", "grep", "glob", "Read", "GREP"] {
            assert!(p.allows(tool), "{tool} should be allowed");
        }
        for tool in [
            "bash",
            "write",
            "edit",
            "web_fetch",
            "mcp__github__create_issue",
            "some-toolbox",
            "",
        ] {
            assert!(!p.allows(tool), "{tool} should be refused");
        }
    }

    #[test]
    fn full_allows_everything() {
        for tool in ["bash", "write", "web_fetch", "mcp__x__y"] {
            assert!(ToolProfile::Full.allows(tool));
        }
    }

    #[test]
    fn cli_allowlist_is_set_only_when_restricted() {
        assert_eq!(ToolProfile::Full.cli_tool_allowlist(), None);
        assert_eq!(
            ToolProfile::ReadOnly.cli_tool_allowlist(),
            Some(vec![
                "Read".to_string(),
                "Grep".to_string(),
                "Glob".to_string()
            ])
        );
    }

    fn access_with_fetch() -> ToolAccess {
        ToolAccess {
            web_fetch: kernex_providers::WebFetchPolicy::AllowAll,
            read_paths: vec![std::path::PathBuf::from("/opt/docs")],
        }
    }

    #[test]
    fn read_only_jobs_never_get_web_fetch() {
        let tools = JobTools::with_access(ToolProfile::ReadOnly, access_with_fetch());
        assert!(!tools.access.web_fetch.is_enabled());
        assert_eq!(tools.access.read_paths, access_with_fetch().read_paths);

        let tools = JobTools::with_access(ToolProfile::Full, access_with_fetch());
        assert_eq!(tools.access, access_with_fetch());
    }

    #[test]
    fn provider_setup_carries_the_profile_allowlist_and_data_dir() {
        let data_dir = Path::new("/srv/kx/.kx/projects/serve");
        let setup = JobTools::default().provider_setup(data_dir);
        assert_eq!(setup.data_dir, data_dir);
        assert_eq!(setup.cli_tools, ToolProfile::ReadOnly.cli_tool_allowlist());
        assert!(!setup.access.web_fetch.is_enabled());

        let full = JobTools::with_access(ToolProfile::Full, ToolAccess::default());
        assert_eq!(full.provider_setup(data_dir).cli_tools, None);
    }

    /// The read tools a read-only job keeps are confined by the runtime:
    /// they reach the working directory and the skills, never the rest of
    /// the disk or the memory database.
    #[test]
    fn read_only_file_tools_stay_inside_the_scope() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("work");
        let data_dir = root.path().join(".kx/projects/serve");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(data_dir.join("skills/demo")).unwrap();
        std::fs::write(workspace.join("notes.md"), "x").unwrap();
        std::fs::write(data_dir.join("memory.db"), "x").unwrap();
        std::fs::write(data_dir.join("skills/demo/SKILL.md"), "x").unwrap();
        std::fs::write(root.path().join("outside.txt"), "x").unwrap();

        let scope = kernex_sandbox::SandboxScope::new(&workspace, &data_dir)
            .with_db_path(data_dir.join("memory.db"));
        let profile = crate::agent_sandbox::sandbox_profile(Vec::new(), &data_dir);
        let read = |p: &Path| kernex_sandbox::tool_read_path(p, &scope, &profile).is_ok();

        assert!(read(&workspace.join("notes.md")));
        assert!(read(&data_dir.join("skills/demo/SKILL.md")));
        assert!(!read(&data_dir.join("memory.db")));
        assert!(!read(&root.path().join("outside.txt")));
    }

    #[tokio::test]
    async fn hook_blocks_a_shell_call_under_read_only() {
        let hook = ServeHookRunner {
            profile: ToolProfile::ReadOnly,
            verbose: false,
        };
        let args = serde_json::json!({ "command": "curl https://example.com" });
        match hook.pre_tool("bash", &args).await {
            HookOutcome::Blocked(reason) => assert!(reason.contains("read-only")),
            HookOutcome::Allow => panic!("bash must be refused under read-only"),
        }
        assert!(matches!(
            hook.pre_tool("read", &args).await,
            HookOutcome::Allow
        ));
    }
}
