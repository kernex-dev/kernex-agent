//! What an agent's tools may reach, for every provider kx builds.
//!
//! The runtime confines the tools to the project directory (the
//! workspace): subprocesses write only there and in the temp and toolchain
//! cache directories, and the in-process file tools (`read`, `write`,
//! `edit`, `grep`, `glob`) refuse every path outside it. The kx data
//! directory, its memory database and the user's `~/.kx/config.toml` stay
//! out of reach. This module holds the few settings that widen that on
//! purpose, and the files kx keeps closed on top of the runtime's list.
//!
//! - [`sandbox_profile`]: the OS sandbox posture (fail-closed), the
//!   credential and shell startup files and the other projects' data
//!   directories no tool may touch, and the extra read-only directories.
//! - [`ToolAccess`]: the `web_fetch` host policy and those read-only
//!   directories, resolved from the command line and the user's
//!   `~/.kx/config.toml`. A project's `.kx.toml` can only turn `web_fetch`
//!   off.

use std::path::{Path, PathBuf};

use kernex_providers::WebFetchPolicy;
use serde::Deserialize;

use crate::config::ProjectConfig;

/// Flags that widen what the tools may reach for one run.
#[derive(clap::Args, Clone, Debug, Default)]
pub struct AccessArgs {
    /// Let the web_fetch tool reach HOST (repeatable or comma-separated;
    /// `*.example.com` covers subdomains). `all` allows any public host and
    /// `none` turns the tool off. Off unless set here or in config, and this
    /// flag replaces the config value. The claude-code and opencode
    /// providers get their own WebFetch and WebSearch only with `all`
    #[arg(
        long = "web-fetch",
        global = true,
        value_name = "HOST|all|none",
        value_delimiter = ',',
        env = "KX_WEB_FETCH"
    )]
    pub web_fetch: Vec<String>,

    /// Extra directory the file tools may read (repeatable). Added to
    /// `read_paths` from config. Writes stay limited to the project
    #[arg(long = "read-path", global = true, value_name = "DIR")]
    pub read_paths: Vec<PathBuf>,

    /// Tools the claude-code CLI may use: `default` (read, search and edit
    /// files, no shell; plus WebFetch and WebSearch when web_fetch is
    /// `all`), `all` (every tool, with the CLI's permission checks off), or a
    /// comma-separated list such as `Read,Edit,Bash(cargo test *)` (a comma
    /// inside parentheses stays in the pattern). opencode runs only with
    /// `all`. Replaces `cli_tools` from config. `kx serve` uses `--tools`
    /// instead
    #[arg(
        long = "cli-tools",
        global = true,
        value_name = "TOOL|all|default",
        env = "KX_CLI_TOOLS"
    )]
    pub cli_tools: Vec<String>,
}

/// `web_fetch` in a config file: `"all"`, `"none"`, or a list of hosts.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum WebFetchSetting {
    Keyword(String),
    Hosts(Vec<String>),
}

/// `~/.kx/config.toml`: the user's own settings, for every project.
///
/// It lives outside the project, so a cloned repository cannot change it,
/// and kx passes its path to the sandbox, so no tool can read or rewrite it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserConfig {
    pub web_fetch: Option<WebFetchSetting>,
    #[serde(default)]
    pub read_paths: Vec<PathBuf>,
    /// Tools the agent CLIs may use. See [`crate::cli_tools`].
    pub cli_tools: Option<crate::cli_tools::CliToolsSetting>,
}

impl UserConfig {
    /// Where the file lives: `config.toml` in the kx root (`~/.kx`).
    pub fn path() -> PathBuf {
        crate::runtime_glue::kx_root().join("config.toml")
    }

    /// Load the user's config. A missing file is the default config.
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&Self::path())
    }

    fn load_from(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
        basic_toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("failed to parse {}: {e}", path.display()))
    }
}

/// Where the tools of one run may reach beyond the project directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolAccess {
    /// Hosts the `web_fetch` tool may reach. Off by default.
    pub web_fetch: WebFetchPolicy,
    /// Extra directories the file tools may read.
    pub read_paths: Vec<PathBuf>,
}

impl ToolAccess {
    /// Resolve the access for a run in `cwd` from the flags and the user's
    /// config. The project's `.kx.toml` (when the run reads one) can only
    /// narrow: `web_fetch = "none"` turns the tool off. Any other
    /// `web_fetch` or `read_paths` there is ignored with a warning, because
    /// that file arrives with a cloned repository.
    ///
    /// For `web_fetch` the flag wins, then a project's `"none"`, then the
    /// user's config. `read_paths` from the user's config and the flags add
    /// up. Relative paths are taken from the current directory for flags
    /// and from the home directory for `~/.kx/config.toml`; `~/` is the
    /// home directory.
    pub fn resolve(
        args: &AccessArgs,
        project: Option<&ProjectConfig>,
        user: &UserConfig,
        cwd: &Path,
    ) -> anyhow::Result<Self> {
        let home = dirs::home_dir().unwrap_or_else(|| cwd.to_path_buf());

        let from_flag = web_fetch_from_flag(&args.web_fetch)?;
        let project_off = project.is_some_and(project_turns_web_fetch_off);
        let from_user = user
            .web_fetch
            .as_ref()
            .map(|s| web_fetch_from_setting(s, "~/.kx/config.toml"))
            .transpose()?;

        let ignored = project.map(ignored_project_access_keys).unwrap_or_default();
        if !ignored.is_empty() {
            warn_ignored_project_access(&ignored);
        }

        let web_fetch = from_flag
            .or(project_off.then_some(WebFetchPolicy::Disabled))
            .or(from_user)
            .unwrap_or_default();
        let mut read_paths: Vec<PathBuf> = user
            .read_paths
            .iter()
            .map(|p| expand(p, &home, &home))
            .collect();
        read_paths.extend(args.read_paths.iter().map(|p| expand(p, cwd, &home)));
        read_paths.dedup();
        Ok(Self {
            web_fetch,
            read_paths,
        })
    }

    /// [`resolve`](Self::resolve) for a run that reads no project file
    /// (`kx serve`): the flags and the user's config only.
    pub fn without_project(args: &AccessArgs) -> anyhow::Result<Self> {
        let cwd = std::env::current_dir()?;
        Self::resolve(args, None, &UserConfig::load()?, &cwd)
    }
}

/// The policy named by `--web-fetch`, or `None` when the flag is not set.
pub fn web_fetch_from_flag(values: &[String]) -> anyhow::Result<Option<WebFetchPolicy>> {
    let values: Vec<&str> = values
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .collect();
    match values.as_slice() {
        [] => Ok(None),
        ["all"] => Ok(Some(WebFetchPolicy::AllowAll)),
        ["none"] => Ok(Some(WebFetchPolicy::Disabled)),
        hosts => host_list(hosts, "--web-fetch").map(Some),
    }
}

/// The policy a config file's `web_fetch` names; `origin` names the file
/// in errors.
pub fn web_fetch_from_setting(
    setting: &WebFetchSetting,
    origin: &str,
) -> anyhow::Result<WebFetchPolicy> {
    match setting {
        WebFetchSetting::Keyword(k) if k == "all" => Ok(WebFetchPolicy::AllowAll),
        WebFetchSetting::Keyword(k) if k == "none" => Ok(WebFetchPolicy::Disabled),
        WebFetchSetting::Keyword(other) => anyhow::bail!(
            "{origin}: web_fetch = {other:?} is not valid; use \"all\", \"none\" or a list \
             of hosts such as [\"docs.rs\"]"
        ),
        WebFetchSetting::Hosts(hosts) => {
            let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
            host_list(&hosts, origin)
        }
    }
}

/// An `AllowHosts` policy for `hosts`, refusing entries that are not host
/// names. An empty list is `Disabled`.
fn host_list(hosts: &[&str], origin: &str) -> anyhow::Result<WebFetchPolicy> {
    if hosts.is_empty() {
        return Ok(WebFetchPolicy::Disabled);
    }
    for host in hosts {
        if *host == "all" || *host == "none" {
            anyhow::bail!("{origin}: `{host}` cannot be combined with host names");
        }
        let bad = host.is_empty()
            || host.contains("://")
            || host.contains('/')
            || host.chars().any(char::is_whitespace);
        if bad {
            anyhow::bail!(
                "{origin}: `{host}` is not a host name; write it without a scheme or path, \
                 for example docs.rs or *.example.com"
            );
        }
    }
    Ok(WebFetchPolicy::AllowHosts(
        hosts.iter().map(|h| h.to_string()).collect(),
    ))
}

/// Whether the project's `.kx.toml` says `web_fetch = "none"`, the one
/// access setting a project file may make: it only narrows.
fn project_turns_web_fetch_off(project: &ProjectConfig) -> bool {
    matches!(&project.web_fetch, Some(WebFetchSetting::Keyword(k)) if k == "none")
}

/// Access keys in the project's `.kx.toml` that kx ignores: anything that
/// would widen what the tools reach.
pub fn ignored_project_access_keys(project: &ProjectConfig) -> Vec<&'static str> {
    let mut ignored = Vec::new();
    if project.web_fetch.is_some() && !project_turns_web_fetch_off(project) {
        ignored.push("web_fetch");
    }
    if project.read_paths.is_some() {
        ignored.push("read_paths");
    }
    ignored
}

/// Tell the user, once per process, that `.kx.toml` asked for more access
/// and kx did not grant it.
fn warn_ignored_project_access(ignored: &[&str]) {
    static WARNED: std::sync::Once = std::sync::Once::new();
    WARNED.call_once(|| {
        eprintln!(
            "warning: .kx.toml sets {}, which kx ignores: a project file cannot widen what \
             the tools reach (it may only set web_fetch = \"none\"). Set them in \
             ~/.kx/config.toml or with --web-fetch / --read-path instead.",
            ignored.join(" and ")
        );
    });
}

/// `path` made absolute: `~/` is the home directory, a relative path is
/// taken from `base`.
fn expand(path: &Path, base: &Path, home: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~") {
        return home.join(rest);
    }
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// Sandbox posture for every provider kx builds: OS-level enforcement is
/// required, never best-effort. A host that cannot apply Seatbelt (macOS)
/// or Landlock (Linux) refuses to spawn agent tool subprocesses instead of
/// silently running them unsandboxed. Embedders of the kernex crates
/// choose their own posture; kx as a product chooses fail-closed.
///
/// `blocked_paths` adds credential stores, shell startup files and every
/// other project's kx data directory to the runtime's own protected list,
/// for subprocesses and the in-process file tools alike. `data_dir` is
/// this run's data directory, which stays as the runtime protects it.
/// `read_paths` are the extra directories the file tools may read; the
/// runtime already confines them to the project otherwise.
pub fn sandbox_profile(
    read_paths: Vec<PathBuf>,
    data_dir: &Path,
) -> kernex_sandbox::SandboxProfile {
    let mut blocked = dirs::home_dir()
        .map(|home| blocked_home_paths(&home))
        .unwrap_or_default();
    blocked.extend(sibling_project_dirs(data_dir));
    kernex_sandbox::SandboxProfile::new()
        .with_require_os_enforcement(true)
        .with_blocked_paths(blocked)
        .with_read_paths(read_paths)
}

/// The other projects' data directories beside `data_dir`: every entry of
/// `~/.kx/projects/` except this run's own. Their memory databases, skills
/// and job history are no business of this run.
///
/// One blocked path per project, read when the provider is built, so the
/// list grows linearly with the number of projects and is not capped. A
/// project created later in the same session is covered from the next
/// start. A data directory that does not sit in a `projects` directory
/// has no siblings to block.
pub(crate) fn sibling_project_dirs(data_dir: &Path) -> Vec<PathBuf> {
    let Some(parent) = data_dir.parent() else {
        return Vec::new();
    };
    if parent.file_name().is_none_or(|n| n != "projects") {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let own = data_dir.file_name();
    let mut siblings: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| Some(e.file_name().as_os_str()) != own)
        .map(|e| e.path())
        .collect();
    siblings.sort();
    siblings
}

/// Credential stores beyond the runtime's own list, relative to `$HOME`.
const EXTRA_CREDENTIAL_PATHS: &[&str] = &[
    ".npmrc",
    ".pypirc",
    ".git-credentials",
    ".cargo/credentials.toml",
    ".cargo/credentials",
    ".azure",
    ".config/op",
    ".terraform.d",
    ".vault-token",
    ".local/share/opencode/auth.json",
];

/// Files a shell or the login session executes on its own. Writing one is
/// code execution outside the sandbox the next time a terminal opens.
const STARTUP_PATHS: &[&str] = &[
    ".zshenv",
    ".zshrc",
    ".zprofile",
    ".zlogin",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".profile",
    ".config/fish",
    "Library/LaunchAgents",
    ".config/autostart",
    ".config/systemd/user",
];

/// Paths under `home` that no agent tool may read or write.
pub(crate) fn blocked_home_paths(home: &Path) -> Vec<PathBuf> {
    let mut paths = kernex_sandbox::credential_read_deny_dirs(home);
    paths.extend(
        EXTRA_CREDENTIAL_PATHS
            .iter()
            .chain(STARTUP_PATHS)
            .map(|p| home.join(p)),
    );
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(web_fetch: &[&str], read_paths: &[&str]) -> AccessArgs {
        AccessArgs {
            web_fetch: web_fetch.iter().map(|s| s.to_string()).collect(),
            read_paths: read_paths.iter().map(PathBuf::from).collect(),
            cli_tools: Vec::new(),
        }
    }

    fn project(toml: &str) -> ProjectConfig {
        basic_toml::from_str(toml).unwrap()
    }

    #[test]
    fn web_fetch_is_off_without_flag_or_config() {
        let access = ToolAccess::resolve(
            &AccessArgs::default(),
            Some(&ProjectConfig::default()),
            &UserConfig::default(),
            Path::new("/work/project"),
        )
        .unwrap();
        assert_eq!(access.web_fetch, WebFetchPolicy::Disabled);
        assert!(!access.web_fetch.is_enabled());
        assert!(access.read_paths.is_empty());
    }

    #[test]
    fn web_fetch_flag_maps_to_the_policy() {
        assert_eq!(web_fetch_from_flag(&[]).unwrap(), None);
        assert_eq!(
            web_fetch_from_flag(&["all".to_string()]).unwrap(),
            Some(WebFetchPolicy::AllowAll)
        );
        assert_eq!(
            web_fetch_from_flag(&["none".to_string()]).unwrap(),
            Some(WebFetchPolicy::Disabled)
        );
        assert_eq!(
            web_fetch_from_flag(&["docs.rs".to_string(), "*.crates.io".to_string()]).unwrap(),
            Some(WebFetchPolicy::AllowHosts(vec![
                "docs.rs".to_string(),
                "*.crates.io".to_string()
            ]))
        );
    }

    #[test]
    fn web_fetch_refuses_urls_and_mixed_keywords() {
        for bad in [
            vec!["https://docs.rs"],
            vec!["docs.rs/std"],
            vec!["all", "docs.rs"],
            vec!["docs.rs", "none"],
        ] {
            let values: Vec<String> = bad.iter().map(|s| s.to_string()).collect();
            assert!(web_fetch_from_flag(&values).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn web_fetch_config_maps_to_the_policy() {
        let config = project("web_fetch = [\"docs.rs\", \"crates.io\"]\n");
        assert_eq!(
            web_fetch_from_setting(config.web_fetch.as_ref().unwrap(), ".kx.toml").unwrap(),
            WebFetchPolicy::AllowHosts(vec!["docs.rs".to_string(), "crates.io".to_string()])
        );
        let config = project("web_fetch = \"all\"\n");
        assert_eq!(
            web_fetch_from_setting(config.web_fetch.as_ref().unwrap(), ".kx.toml").unwrap(),
            WebFetchPolicy::AllowAll
        );
        let config = project("web_fetch = \"none\"\n");
        assert_eq!(
            web_fetch_from_setting(config.web_fetch.as_ref().unwrap(), ".kx.toml").unwrap(),
            WebFetchPolicy::Disabled
        );
        let config = project("web_fetch = []\n");
        assert_eq!(
            web_fetch_from_setting(config.web_fetch.as_ref().unwrap(), ".kx.toml").unwrap(),
            WebFetchPolicy::Disabled
        );
        let config = project("web_fetch = \"docs.rs\"\n");
        assert!(web_fetch_from_setting(config.web_fetch.as_ref().unwrap(), ".kx.toml").is_err());
    }

    #[test]
    fn web_fetch_flag_beats_user_config() {
        let user: UserConfig = basic_toml::from_str("web_fetch = \"all\"\n").unwrap();
        let cwd = Path::new("/work/project");

        let only_user = ToolAccess::resolve(&AccessArgs::default(), None, &user, cwd).unwrap();
        assert_eq!(only_user.web_fetch, WebFetchPolicy::AllowAll);

        let with_flag = ToolAccess::resolve(&args(&["docs.rs"], &[]), None, &user, cwd).unwrap();
        assert_eq!(
            with_flag.web_fetch,
            WebFetchPolicy::AllowHosts(vec!["docs.rs".to_string()])
        );

        let flag_off = ToolAccess::resolve(&args(&["none"], &[]), None, &user, cwd).unwrap();
        assert_eq!(flag_off.web_fetch, WebFetchPolicy::Disabled);
    }

    /// A cloned repository ships its own `.kx.toml`: it must not turn
    /// web_fetch on or add read roots, whatever it says.
    #[test]
    fn project_file_cannot_widen_access() {
        let cwd = Path::new("/work/project");
        let none = UserConfig::default();
        for toml in [
            "web_fetch = \"all\"\n",
            "web_fetch = [\"attacker.example\"]\n",
            "web_fetch = \"bogus\"\n",
            "read_paths = [\"/\", \"~\"]\n",
        ] {
            let config = project(toml);
            let access =
                ToolAccess::resolve(&AccessArgs::default(), Some(&config), &none, cwd).unwrap();
            assert_eq!(access, ToolAccess::default(), "{toml}");
            assert!(!ignored_project_access_keys(&config).is_empty(), "{toml}");
        }

        // The user's own opt-in still applies beside an ignored project key.
        let user: UserConfig =
            basic_toml::from_str("web_fetch = [\"docs.rs\"]\nread_paths = [\"/opt/d\"]\n").unwrap();
        let config = project("web_fetch = \"all\"\nread_paths = [\"/\"]\n");
        let access =
            ToolAccess::resolve(&AccessArgs::default(), Some(&config), &user, cwd).unwrap();
        assert_eq!(
            access.web_fetch,
            WebFetchPolicy::AllowHosts(vec!["docs.rs".to_string()])
        );
        assert_eq!(access.read_paths, vec![PathBuf::from("/opt/d")]);
        assert_eq!(
            ignored_project_access_keys(&config),
            vec!["web_fetch", "read_paths"]
        );
    }

    /// `web_fetch = "none"` only narrows, so a project file may set it.
    #[test]
    fn project_file_may_turn_web_fetch_off() {
        let cwd = Path::new("/work/project");
        let user: UserConfig = basic_toml::from_str("web_fetch = \"all\"\n").unwrap();
        let config = project("web_fetch = \"none\"\n");
        assert!(ignored_project_access_keys(&config).is_empty());

        let access =
            ToolAccess::resolve(&AccessArgs::default(), Some(&config), &user, cwd).unwrap();
        assert_eq!(access.web_fetch, WebFetchPolicy::Disabled);

        // The flag is the user's own choice for this run and still wins.
        let access =
            ToolAccess::resolve(&args(&["docs.rs"], &[]), Some(&config), &user, cwd).unwrap();
        assert!(access.web_fetch.is_enabled());
    }

    #[test]
    fn read_paths_add_up_and_become_absolute() {
        let user: UserConfig =
            basic_toml::from_str("read_paths = [\"/opt/shared-docs\"]\n").unwrap();
        let cwd = Path::new("/work/project");
        let access =
            ToolAccess::resolve(&args(&[], &["notes", "/abs/dir"]), None, &user, cwd).unwrap();
        assert_eq!(
            access.read_paths,
            vec![
                PathBuf::from("/opt/shared-docs"),
                PathBuf::from("/work/project/notes"),
                PathBuf::from("/abs/dir"),
            ]
        );
        assert!(access.read_paths.iter().all(|p| p.is_absolute()));
    }

    #[test]
    fn home_relative_read_paths_expand() {
        let home = Path::new("/home/someone");
        assert_eq!(
            expand(Path::new("~/src/lib"), Path::new("/x"), home),
            PathBuf::from("/home/someone/src/lib")
        );
    }

    #[test]
    fn user_config_rejects_unknown_keys_and_reads_missing_as_default() {
        assert!(basic_toml::from_str::<UserConfig>("provider = \"x\"\n").is_err());
        let dir = tempfile::tempdir().unwrap();
        let config = UserConfig::load_from(&dir.path().join("config.toml")).unwrap();
        assert!(config.web_fetch.is_none());
        assert!(config.read_paths.is_empty());
    }

    #[test]
    fn sandbox_profile_requires_os_enforcement_and_denies_egress() {
        // kx's fail-closed posture: every provider gets a profile that
        // refuses unsandboxed tool subprocesses. If this flag ever flips
        // back to best-effort it must be a deliberate, reviewed change.
        let profile = sandbox_profile(
            vec![PathBuf::from("/opt/docs")],
            Path::new("/nonexistent/.kx/projects/demo"),
        );
        assert!(profile.require_os_enforcement);
        assert!(!profile.allow_network, "egress must default to deny");
        assert_eq!(profile.read_paths, vec![PathBuf::from("/opt/docs")]);
        if dirs::home_dir().is_some() {
            assert!(!profile.blocked_paths.is_empty());
        }
    }

    /// Two projects side by side: a run in one cannot reach the other's
    /// memory, skills or config, and still reads its own skills.
    #[test]
    fn other_projects_data_dirs_are_blocked() {
        let root = tempfile::tempdir().unwrap();
        let projects = root.path().join(".kx/projects");
        let own = projects.join("alpha");
        let other = projects.join("beta");
        for dir in [&own, &other] {
            std::fs::create_dir_all(dir.join("skills/demo")).unwrap();
            std::fs::write(dir.join("skills/demo/SKILL.md"), "x").unwrap();
            std::fs::write(dir.join("memory.db"), "x").unwrap();
            std::fs::write(dir.join("skills.toml"), "x").unwrap();
        }
        let workspace = root.path().join("work/alpha");
        std::fs::create_dir_all(&workspace).unwrap();

        assert_eq!(sibling_project_dirs(&own), vec![other.clone()]);
        let profile = sandbox_profile(Vec::new(), &own);
        assert!(profile.blocked_paths.contains(&other));
        assert!(!profile.blocked_paths.contains(&own));

        let scope =
            kernex_sandbox::SandboxScope::new(&workspace, &own).with_db_path(own.join("memory.db"));
        let tool_read = |p: &Path| kernex_sandbox::tool_read_path(p, &scope, &profile).is_ok();
        let shell_read = |p: &Path| !kernex_sandbox::is_read_blocked(p, &scope, &profile);

        assert!(tool_read(&own.join("skills/demo/SKILL.md")));
        for path in ["memory.db", "skills/demo/SKILL.md", "skills.toml"] {
            assert!(!tool_read(&other.join(path)), "{path}");
            assert!(!shell_read(&other.join(path)), "{path}");
        }
    }

    #[test]
    fn a_data_dir_outside_a_projects_dir_blocks_no_siblings() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("neighbour")).unwrap();
        assert!(sibling_project_dirs(&root.path().join("state")).is_empty());
    }

    /// The file tools and subprocesses must not reach the user's keys or
    /// rewrite a shell startup file, even inside a broad read root.
    #[test]
    fn tools_cannot_reach_credentials_or_startup_files() {
        let home = Path::new("/home/someone");
        let profile = kernex_sandbox::SandboxProfile::new()
            .with_blocked_paths(blocked_home_paths(home))
            .with_read_paths(vec![home.to_path_buf()]);
        let scope = kernex_sandbox::SandboxScope::new(
            home.join("code/project"),
            home.join(".kx/projects/project"),
        );
        for secret in [
            ".ssh/id_ed25519",
            ".aws/credentials",
            ".config/gh/hosts.yml",
            ".npmrc",
            ".cargo/credentials.toml",
            ".git-credentials",
        ] {
            assert!(
                kernex_sandbox::is_read_blocked(&home.join(secret), &scope, &profile),
                "{secret} must not be readable"
            );
            assert!(
                kernex_sandbox::tool_read_path(&home.join(secret), &scope, &profile).is_err(),
                "{secret} must not be readable by the file tools"
            );
        }
        for startup in [
            ".zshrc",
            ".zshenv",
            ".bashrc",
            ".profile",
            ".ssh/authorized_keys",
        ] {
            assert!(
                kernex_sandbox::is_write_blocked(&home.join(startup), &scope, &profile),
                "{startup} must not be writable"
            );
        }
        // The project itself stays usable.
        let source = home.join("code/project/src/main.rs");
        assert!(!kernex_sandbox::is_read_blocked(&source, &scope, &profile));
        assert!(!kernex_sandbox::is_write_blocked(&source, &scope, &profile));
    }
}
