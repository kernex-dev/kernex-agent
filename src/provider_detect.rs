//! Which provider kx uses when none is named.
//!
//! kx has no default provider. `--provider` (or `KERNEX_PROVIDER`) wins;
//! without it kx picks from what is configured, in this order, and says on
//! stderr which one it chose and why:
//!
//! 1. an API key in the environment, in the order of
//!    [`PROVIDERS`](crate::runtime_glue::PROVIDERS);
//! 2. an Ollama server that answers at `OLLAMA_HOST` (default
//!    `127.0.0.1:11434`);
//! 3. an agent CLI on `PATH`: `claude`, then `opencode`.
//!
//! When nothing is found the command stops with the ways to configure one.
//! Commands that never call a model do not look. A named `ollama` without
//! a base URL also reads `OLLAMA_HOST`. `KERNEX_API_KEY` (or `--api-key`)
//! is used only with a named provider: it says nothing about which one.

use std::path::Path;
use std::time::Duration;

use crate::cli::{Cli, Command, PipelineAction};
use crate::runtime_glue::{ProviderFlags, PROVIDERS};

/// Where Ollama listens unless `OLLAMA_HOST` says otherwise.
const OLLAMA_DEFAULT: &str = "http://127.0.0.1:11434";

/// Agent CLIs kx can drive, in the order they are tried.
const CLIS: &[(&str, &str)] = &[("claude", "claude-code"), ("opencode", "opencode")];

/// The provider kx will use and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub name: String,
    /// Base URL to use when the choice found it (Ollama at `OLLAMA_HOST`).
    pub base_url: Option<String>,
    /// Why this provider, for the notice; `None` when the user named it.
    pub reason: Option<String>,
}

/// What [`choose_with`] may look at. Tests replace the real lookups.
pub struct Probe<'a> {
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub ollama_answers: &'a dyn Fn(&str) -> bool,
    pub on_path: &'a dyn Fn(&str) -> bool,
}

/// The provider flags for this run: the provider [`for_command`] gives, and
/// the rest as parsed. An empty model, key or base URL counts as unset: the
/// compose files pass `KERNEX_MODEL`, `KERNEX_API_KEY` and `KERNEX_BASE_URL`
/// empty when `.env` leaves them out.
pub fn provider_flags(cli: &Cli) -> anyhow::Result<ProviderFlags> {
    let choice = for_command(cli.command.as_ref(), cli.provider.as_deref())?;
    Ok(flags_for(cli, choice))
}

fn flags_for(cli: &Cli, choice: Choice) -> ProviderFlags {
    let set = |v: &Option<String>| v.clone().filter(|s| !s.trim().is_empty());
    ProviderFlags {
        name: choice.name,
        model: set(&cli.model),
        // A key without a provider name belongs to no provider in particular,
        // so a provider kx picked uses its own key variable.
        api_key: set(&cli.api_key).filter(|_| choice.reason.is_none()),
        base_url: set(&cli.base_url).or(choice.base_url),
        project: cli.project.clone(),
        channel: cli.channel.clone(),
        max_tokens: cli.max_tokens,
        no_memory: cli.no_memory,
        auto_compact: !cli.no_auto_compact,
        verbose: cli.verbose,
        access: cli.access.clone(),
    }
}

/// The provider for `command`, with the notice printed when kx chose it.
/// Commands that call no model get the named provider or an empty name.
/// `kx doctor` reports a failed choice instead of stopping.
pub fn for_command(command: Option<&Command>, named: Option<&str>) -> anyhow::Result<Choice> {
    let unnamed = || Choice {
        name: named.unwrap_or_default().trim().to_string(),
        base_url: None,
        reason: None,
    };
    if !needs_provider(command) {
        return Ok(unnamed());
    }
    match choose(named) {
        Ok(choice) => {
            if let Some(reason) = &choice.reason {
                eprintln!(
                    "kx: using provider {} ({reason}); pass --provider or set \
                     KERNEX_PROVIDER to pick another",
                    choice.name
                );
            }
            Ok(choice)
        }
        Err(e) if matches!(command, Some(Command::Doctor)) => {
            eprintln!("kx: {e}");
            Ok(unnamed())
        }
        Err(e) => Err(e),
    }
}

/// Whether `command` calls a model. The one-shot prompt and the REPL
/// (no subcommand) do.
fn needs_provider(command: Option<&Command>) -> bool {
    match command {
        None => true,
        Some(c) => {
            matches!(
                c,
                Command::Dev { .. }
                    | Command::Audit
                    | Command::Docs
                    | Command::Doctor
                    | Command::Pipeline {
                        action: PipelineAction::Run { .. }
                    }
            ) || is_serve(c)
        }
    }
}

#[cfg(feature = "serve")]
fn is_serve(c: &Command) -> bool {
    matches!(c, Command::Serve { .. })
}

#[cfg(not(feature = "serve"))]
fn is_serve(_: &Command) -> bool {
    false
}

/// [`choose_with`] against the real environment, Ollama and `PATH`.
pub fn choose(named: Option<&str>) -> anyhow::Result<Choice> {
    let env = |k: &str| std::env::var(k).ok();
    choose_with(
        named,
        &Probe {
            env: &env,
            ollama_answers: &ollama_answers,
            on_path: &on_path,
        },
    )
}

/// The provider `named`, or the first one configured. See the module docs
/// for the order.
pub fn choose_with(named: Option<&str>, probe: &Probe<'_>) -> anyhow::Result<Choice> {
    let host = (probe.env)("OLLAMA_HOST").filter(|h| !h.trim().is_empty());
    let url = host
        .as_deref()
        .map(ollama_url)
        .unwrap_or_else(|| OLLAMA_DEFAULT.to_string());
    if let Some(name) = named.map(str::trim).filter(|n| !n.is_empty()) {
        let ollama = name.eq_ignore_ascii_case("ollama");
        return Ok(Choice {
            name: name.to_string(),
            base_url: host.as_ref().filter(|_| ollama).map(|_| url.clone()),
            reason: None,
        });
    }
    // Present and not empty, the rule the provider's own key lookup uses.
    let set = |var: &str| (probe.env)(var).is_some_and(|v| !v.is_empty());

    let keyed: Vec<(&str, &str)> = PROVIDERS
        .iter()
        .filter_map(|p| p.api_key_env.map(|var| (p.name, var)))
        .filter(|(_, var)| set(var))
        .collect();
    if let Some((name, var)) = keyed.first() {
        let mut reason = format!("{var} is set");
        if keyed.len() > 1 {
            let others: Vec<&str> = keyed[1..].iter().map(|(_, v)| *v).collect();
            reason.push_str(&format!("; also set: {}", others.join(", ")));
        }
        return Ok(Choice {
            name: (*name).to_string(),
            base_url: None,
            reason: Some(reason),
        });
    }

    if (probe.ollama_answers)(&url) {
        return Ok(Choice {
            name: "ollama".into(),
            base_url: host.is_some().then(|| url.clone()),
            reason: Some(format!("Ollama answers at {url}")),
        });
    }

    for (program, name) in CLIS {
        if (probe.on_path)(program) {
            return Ok(Choice {
                name: (*name).to_string(),
                base_url: None,
                reason: Some(format!("the {program} CLI is on PATH")),
            });
        }
    }

    let vars: Vec<&str> = PROVIDERS.iter().filter_map(|p| p.api_key_env).collect();
    let mut msg = format!(
        "no provider is configured. Set an API key ({}), start Ollama ({url}), install the \
         claude or opencode CLI, or pass --provider <name>",
        vars.join(", ")
    );
    if set("KERNEX_API_KEY") {
        msg.push_str(". KERNEX_API_KEY is set but names no provider: pass --provider with it");
    }
    anyhow::bail!(msg)
}

/// `OLLAMA_HOST` as a URL, read the way Ollama reads it: `host` or
/// `host:port` is plain HTTP on port 11434 by default, while a URL with
/// `http://` or `https://` defaults to port 80 or 443. A path is dropped.
fn ollama_url(host: &str) -> String {
    let host = host.trim();
    let (scheme, rest) = match host.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => (String::new(), host),
    };
    let default_port = match scheme.as_str() {
        "" => "11434",
        "https" => "443",
        _ => "80",
    };
    let scheme = if scheme.is_empty() {
        "http".to_string()
    } else {
        scheme
    };
    let authority = rest.split('/').next().unwrap_or_default();
    // Credentials are never shown or sent: kx prints this address.
    let hostport = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = split_port(hostport);
    // A bare IPv6 address needs brackets in a URL.
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    format!("{scheme}://{host}:{}", port.unwrap_or(default_port))
}

/// `hostport` split into host and a non-empty numeric port. A bare IPv6
/// address (more than one colon, no brackets) has no port.
fn split_port(hostport: &str) -> (&str, Option<&str>) {
    let digits = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit());
    if hostport.starts_with('[') {
        return match hostport.split_once(']') {
            Some((addr, after)) => (
                &hostport[..addr.len() + 1],
                after.strip_prefix(':').filter(|p| digits(p)),
            ),
            None => (hostport, None),
        };
    }
    if hostport.matches(':').count() > 1 {
        return (hostport, None);
    }
    match hostport.split_once(':') {
        Some((h, p)) => (h, Some(p).filter(|p| digits(p))),
        None => (hostport, None),
    }
}

/// Whether an Ollama server answers `GET {url}/api/tags` within a second.
fn ollama_answers(url: &str) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(300))
        .timeout(Duration::from_secs(1))
        .build();
    agent.get(&format!("{url}/api/tags")).call().is_ok()
}

/// Whether `program` is an executable file in a `PATH` directory.
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| is_executable(&dir.join(program)))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn run(
        named: Option<&str>,
        vars: &[(&str, &str)],
        ollama: bool,
        clis: &[&str],
    ) -> anyhow::Result<Choice> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let env = move |k: &str| vars.get(k).cloned();
        let ollama_answers = move |_: &str| ollama;
        let clis: Vec<String> = clis.iter().map(|c| c.to_string()).collect();
        let on_path = move |p: &str| clis.iter().any(|c| c == p);
        choose_with(
            named,
            &Probe {
                env: &env,
                ollama_answers: &ollama_answers,
                on_path: &on_path,
            },
        )
    }

    #[test]
    fn a_named_provider_wins_over_everything() {
        let c = run(
            Some(" ollama "),
            &[("ANTHROPIC_API_KEY", "k")],
            true,
            &["claude"],
        )
        .unwrap();
        assert_eq!(c.name, "ollama");
        assert_eq!(c.reason, None);
    }

    #[test]
    fn an_empty_name_is_no_name() {
        let c = run(Some("  "), &[], false, &["claude"]).unwrap();
        assert_eq!(c.name, "claude-code");
    }

    #[test]
    fn an_api_key_comes_first_in_table_order() {
        let c = run(
            None,
            &[("OPENAI_API_KEY", "k"), ("ANTHROPIC_API_KEY", "k")],
            true,
            &["claude"],
        )
        .unwrap();
        assert_eq!(c.name, "anthropic");
        let reason = c.reason.unwrap();
        assert!(reason.contains("ANTHROPIC_API_KEY is set"), "{reason}");
        assert!(reason.contains("also set: OPENAI_API_KEY"), "{reason}");
    }

    #[test]
    fn an_empty_key_does_not_count() {
        let c = run(None, &[("ANTHROPIC_API_KEY", "")], false, &["claude"]).unwrap();
        assert_eq!(c.name, "claude-code");
    }

    #[test]
    fn a_named_ollama_reads_ollama_host_without_probing() {
        let env = |k: &str| (k == "OLLAMA_HOST").then(|| "box:9999".to_string());
        let probe = Probe {
            env: &env,
            ollama_answers: &|_| panic!("a named provider must not probe"),
            on_path: &|_| panic!("a named provider must not search PATH"),
        };
        let c = choose_with(Some("Ollama"), &probe).unwrap();
        assert_eq!(c.base_url.as_deref(), Some("http://box:9999"));
        // Never applied to another provider.
        let c = choose_with(Some("openai"), &probe).unwrap();
        assert_eq!(c.base_url, None);
    }

    #[test]
    fn ollama_comes_before_an_installed_cli() {
        let c = run(None, &[], true, &["claude"]).unwrap();
        assert_eq!(c.name, "ollama");
        assert_eq!(c.base_url, None, "the default address needs no base URL");
        let c = run(None, &[("OLLAMA_HOST", "box:9999")], true, &[]).unwrap();
        assert_eq!(c.base_url.as_deref(), Some("http://box:9999"));
    }

    #[test]
    fn claude_comes_before_opencode() {
        assert_eq!(
            run(None, &[], false, &["opencode", "claude"]).unwrap().name,
            "claude-code"
        );
        assert_eq!(
            run(None, &[], false, &["opencode"]).unwrap().name,
            "opencode"
        );
    }

    #[test]
    fn nothing_configured_says_how_to_configure_one() {
        let e = run(None, &[], false, &[]).unwrap_err().to_string();
        assert!(e.contains("no provider is configured"), "{e}");
        assert!(e.contains("ANTHROPIC_API_KEY"), "{e}");
        assert!(e.contains("--provider"), "{e}");
        assert!(!e.contains("KERNEX_API_KEY"), "{e}");
        let e = run(None, &[("KERNEX_API_KEY", "k")], false, &[])
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("KERNEX_API_KEY is set but names no provider"),
            "{e}"
        );
    }

    #[test]
    fn ollama_host_forms() {
        assert_eq!(ollama_url("127.0.0.1:11434"), "http://127.0.0.1:11434");
        assert_eq!(ollama_url("box"), "http://box:11434");
        assert_eq!(ollama_url(" 0.0.0.0 "), "http://0.0.0.0:11434");
        assert_eq!(ollama_url("http://box"), "http://box:80");
        assert_eq!(
            ollama_url("https://ollama.example/v1"),
            "https://ollama.example:443"
        );
        assert_eq!(ollama_url("http://box:1234"), "http://box:1234");
        assert_eq!(ollama_url("HTTPS://box"), "https://box:443");
        assert_eq!(ollama_url("http://box:/x"), "http://box:80");
        assert_eq!(ollama_url("::1"), "http://[::1]:11434");
        assert_eq!(ollama_url("[::1]:8080"), "http://[::1]:8080");
        assert_eq!(ollama_url("http://[::1]"), "http://[::1]:80");
        assert_eq!(ollama_url("user:pw@box:7"), "http://box:7");
    }

    #[test]
    fn commands_that_call_no_model_do_not_look() {
        assert!(needs_provider(None));
        assert!(needs_provider(Some(&Command::Doctor)));
        assert!(!needs_provider(Some(&Command::Init)));
        assert!(!needs_provider(Some(&Command::Pipeline {
            action: PipelineAction::List
        })));
        assert!(needs_provider(Some(&Command::Pipeline {
            action: PipelineAction::Run { name: "x".into() }
        })));
        let c = for_command(Some(&Command::Init), None).unwrap();
        assert_eq!(c.name, "");
    }

    #[test]
    fn empty_values_count_as_unset_and_a_key_needs_a_name() {
        use clap::Parser;
        let cli =
            Cli::try_parse_from(["kx", "--model=", "--api-key=", "--base-url=", "init"]).unwrap();
        let flags = provider_flags(&cli).unwrap();
        assert_eq!(flags.model, None);
        assert_eq!(flags.api_key, None);
        assert_eq!(flags.base_url, None);
        let cli =
            Cli::try_parse_from(["kx", "--provider", "openai", "--api-key", "k", "init"]).unwrap();
        assert_eq!(provider_flags(&cli).unwrap().api_key.as_deref(), Some("k"));
    }

    #[test]
    fn a_provider_kx_picked_does_not_get_the_unnamed_key() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["kx", "--api-key", "k", "--base-url", "http://u"]).unwrap();
        let picked = Choice {
            name: "openai".into(),
            base_url: None,
            reason: Some("OPENAI_API_KEY is set".into()),
        };
        let flags = flags_for(&cli, picked);
        assert_eq!(flags.name, "openai");
        assert_eq!(flags.api_key, None);
        assert_eq!(flags.base_url.as_deref(), Some("http://u"));

        let named = Choice {
            name: "openai".into(),
            base_url: None,
            reason: None,
        };
        assert_eq!(flags_for(&cli, named).api_key.as_deref(), Some("k"));
    }
}
