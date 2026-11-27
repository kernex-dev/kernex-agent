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
//! a base URL also reads `OLLAMA_HOST`, and uses the same default address.
//! `KERNEX_API_KEY` (or `--api-key`)
//! is used only with a named provider: it says nothing about which one.

use std::path::Path;
use std::time::Duration;

use colored::Colorize;

use crate::cli::{Cli, Command, PipelineAction};
use crate::runtime_glue::{ProviderFlags, ProviderSpec, PROVIDERS};

/// Where Ollama listens unless `OLLAMA_HOST` says otherwise.
const OLLAMA_DEFAULT: &str = "http://127.0.0.1:11434";

/// Agent CLIs kx can drive, in the order they are tried.
const CLIS: &[(&str, &str)] = &[("claude", "claude-code"), ("opencode", "opencode")];

/// The provider kx will use and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub name: String,
    /// Base URL for Ollama: `OLLAMA_HOST`, else `127.0.0.1:11434`, the
    /// address the search and `kx doctor` probe. `None` for the rest.
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
    if serve_ignores_openai_api(cli) {
        eprintln!(
            "warning: kx serve jobs ignore --openai-api / KERNEX_OPENAI_API and use the API their base URL implies"
        );
    }
    Ok(flags_for(cli, choice))
}

/// Whether this is `kx serve` with `--openai-api` set: its jobs do not read it.
fn serve_ignores_openai_api(cli: &Cli) -> bool {
    cli.openai_api().is_some() && cli.command.as_ref().is_some_and(is_serve)
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
        openai_api: cli.openai_api(),
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
            base_url: ollama.then(|| url.clone()),
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
            base_url: Some(url.clone()),
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

/// What `kx doctor` shows for `spec`: where its API key comes from, or for
/// a provider without one, whether it is there.
pub fn doctor_status(spec: &ProviderSpec, flags: &ProviderFlags) -> String {
    let Some(var) = spec.api_key_env else {
        return keyless_status(spec.name, flags);
    };
    let env = |k: &str| std::env::var(k).ok();
    match key_source(spec.name, var, flags, &env) {
        Some(source) => format!("{} via {source}", "set".green()),
        None => format!("{} ({var} unset)", "missing".yellow()),
    }
}

/// Where the key for provider `name` comes from, by the rules the key
/// lookup uses: the flags' key, which only the active provider gets, then
/// `var` when not empty.
fn key_source(
    name: &str,
    var: &str,
    flags: &ProviderFlags,
    env: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    if flags.api_key.is_some() && flags.name.trim().eq_ignore_ascii_case(name) {
        Some("--api-key or KERNEX_API_KEY".to_string())
    } else {
        env(var).filter(|v| !v.is_empty()).map(|_| var.to_string())
    }
}

/// Whether a provider that needs no API key is there: whether Ollama
/// answers, or whether the agent CLI is on `PATH`, checked the way the
/// provider search checks them. For the active provider, a `--base-url` is
/// where Ollama is looked for.
fn keyless_status(name: &str, flags: &ProviderFlags) -> String {
    let env = |k: &str| std::env::var(k).ok();
    let probe = Probe {
        env: &env,
        ollama_answers: &ollama_answers,
        on_path: &on_path,
    };
    match keyless_status_with(name, flags, &probe) {
        Some((true, what)) => format!("{} ({what})", "found".green()),
        Some((false, what)) => format!("{} ({what})", "missing".yellow()),
        None => "(no API key required)".dimmed().to_string(),
    }
}

/// Whether the keyless provider `name` is there, and how it was looked
/// for. `None` for a provider this check does not know.
fn keyless_status_with(
    name: &str,
    flags: &ProviderFlags,
    probe: &Probe<'_>,
) -> Option<(bool, String)> {
    if name == "ollama" {
        // The provider uses `--base-url` as given, so it is probed as given.
        let base_url = Some(flags.base_url.as_deref().unwrap_or_default().trim())
            .filter(|u| !u.is_empty() && flags.name.trim().eq_ignore_ascii_case(name))
            .map(|u| u.trim_end_matches('/').to_string());
        let url = base_url.unwrap_or_else(|| {
            (probe.env)("OLLAMA_HOST")
                .filter(|h| !h.trim().is_empty())
                .map(|h| ollama_url(&h))
                .unwrap_or_else(|| OLLAMA_DEFAULT.to_string())
        });
        let shown =
            printable(&url).unwrap_or_else(|| "an address that is not an http URL".to_string());
        // This check connects directly; the provider's client honours the
        // proxy variables, so with one set the two can disagree.
        let proxy = proxy_var(&url, probe.env).map_or_else(String::new, |var| {
            format!(
                "; checked without the proxy in {var}, which the provider uses \
                 unless NO_PROXY exempts this address"
            )
        });
        return Some(if (probe.ollama_answers)(&url) {
            (true, format!("answers at {shown}{proxy}"))
        } else {
            (false, format!("nothing answers at {shown}{proxy}"))
        });
    }
    let (program, _) = CLIS.iter().find(|(_, n)| *n == name)?;
    Some(if (probe.on_path)(program) {
        (true, format!("{program} on PATH"))
    } else {
        (false, format!("{program} not on PATH"))
    })
}

/// The proxy variable the provider's HTTP client reads for `url`, when it
/// holds a proxy the client accepts: `HTTPS_PROXY` or `HTTP_PROXY` by
/// scheme, each before its lowercase form, then `ALL_PROXY` and
/// `all_proxy`. The first of each pair that is set is the one read.
fn proxy_var(url: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<&'static str> {
    let first = |names: [&'static str; 2]| {
        names
            .into_iter()
            .find(|n| env(n).is_some())
            .filter(|n| env(n).is_some_and(|v| accepted_proxy(&v)))
    };
    let by_scheme = if url
        .get(..6)
        .is_some_and(|s| s.eq_ignore_ascii_case("https:"))
    {
        ["HTTPS_PROXY", "https_proxy"]
    } else {
        ["HTTP_PROXY", "http_proxy"]
    };
    first(by_scheme).or_else(|| first(["ALL_PROXY", "all_proxy"]))
}

/// Whether the client would use `value` as a proxy, by the test its proxy
/// matcher applies (hyper-util's `parse_env_uri`): it parses as a URI with
/// a host, and its scheme is http, https, a lowercase socks scheme, or
/// absent. The client ignores any other value.
fn accepted_proxy(value: &str) -> bool {
    use http::uri::{Scheme, Uri};
    let Ok(uri) = value.parse::<Uri>() else {
        return false;
    };
    let scheme_ok = match uri.scheme() {
        None => true,
        Some(s) => {
            *s == Scheme::HTTP
                || *s == Scheme::HTTPS
                || matches!(s.as_str(), "socks4" | "socks4a" | "socks5" | "socks5h")
        }
    };
    scheme_ok && uri.authority().is_some()
}

/// `url` as `kx doctor` prints it: an http or https URL, parsed the way
/// the provider's HTTP client parses it, without its user, password, query
/// or fragment. `None` when `url` is not one, since nothing then tells
/// where a password in it ends.
fn printable(url: &str) -> Option<String> {
    let mut url = url::Url::parse(url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https") && u.has_host())?;
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    url.set_query(None);
    url.set_fragment(None);
    Some(url.as_str().trim_end_matches('/').to_string())
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
        // Without OLLAMA_HOST, the address kx doctor probes.
        let c = run(Some("ollama"), &[], false, &[]).unwrap();
        assert_eq!(c.base_url.as_deref(), Some(OLLAMA_DEFAULT));
    }

    #[test]
    fn ollama_comes_before_an_installed_cli() {
        let c = run(None, &[], true, &["claude"]).unwrap();
        assert_eq!(c.name, "ollama");
        assert_eq!(
            c.base_url.as_deref(),
            Some(OLLAMA_DEFAULT),
            "the provider must use the address that answered, not its own default"
        );
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

    #[test]
    fn the_openai_api_flag_reaches_the_provider_flags() {
        use clap::Parser;
        use kernex_providers::openai::OpenAiApi;
        let named = || Choice {
            name: "openai".into(),
            base_url: None,
            reason: None,
        };
        let cli = Cli::try_parse_from(["kx", "--openai-api", "chat", "init"]).unwrap();
        assert_eq!(
            flags_for(&cli, named()).openai_api,
            Some(OpenAiApi::ChatCompletions)
        );
        let cli = Cli::try_parse_from(["kx", "init"]).unwrap();
        assert_eq!(flags_for(&cli, named()).openai_api, None);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn serve_says_it_ignores_the_openai_api_flag() {
        use clap::Parser;
        let parse = |args: &[&str]| Cli::try_parse_from(args).unwrap();
        assert!(serve_ignores_openai_api(&parse(&[
            "kx",
            "--openai-api",
            "responses",
            "serve"
        ])));
        assert!(!serve_ignores_openai_api(&parse(&["kx", "serve"])));
        assert!(!serve_ignores_openai_api(&parse(&[
            "kx",
            "--openai-api",
            "responses",
            "dev"
        ])));
    }

    #[test]
    fn the_proxy_variable_is_the_one_the_provider_client_reads() {
        let with = |vars: &[(&str, &str)], url: &str| {
            let vars: HashMap<String, String> = vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            proxy_var(url, &move |k: &str| vars.get(k).cloned())
        };
        let http = "http://127.0.0.1:11434";
        assert_eq!(with(&[], http), None);
        assert_eq!(
            with(&[("HTTP_PROXY", "http://p")], http),
            Some("HTTP_PROXY")
        );
        assert_eq!(
            with(&[("http_proxy", "http://p")], http),
            Some("http_proxy")
        );
        assert_eq!(with(&[("all_proxy", "http://p")], http), Some("all_proxy"));
        // An https proxy is not used for an http address, and the other way round.
        assert_eq!(with(&[("HTTPS_PROXY", "http://p")], http), None);
        assert_eq!(
            with(&[("HTTPS_PROXY", "http://p")], "HTTPS://box"),
            Some("HTTPS_PROXY")
        );
        assert_eq!(with(&[("HTTP_PROXY", "http://p")], "https://box"), None);
        // An empty HTTP_PROXY counts as unset and hides http_proxy, as in
        // the client; ALL_PROXY still applies.
        let empty = [("HTTP_PROXY", " "), ("http_proxy", "http://p")];
        assert_eq!(with(&empty, http), None);
        let all = [("HTTP_PROXY", ""), ("ALL_PROXY", "http://p")];
        assert_eq!(with(&all, http), Some("ALL_PROXY"));
        // A value the client rejects is skipped the same way.
        assert_eq!(
            with(&[("HTTP_PROXY", "proxy:3128")], http),
            Some("HTTP_PROXY")
        );
        assert_eq!(
            with(&[("HTTP_PROXY", "socks5://p")], http),
            Some("HTTP_PROXY")
        );
        assert_eq!(
            with(&[("HTTP_PROXY", "HTTP://p:3128")], http),
            Some("HTTP_PROXY")
        );
        let user = [("HTTP_PROXY", "http://user:p@ss@h:8080")];
        assert_eq!(with(&user, http), Some("HTTP_PROXY"));
        for bad in [
            "ftp://p",
            "http://",
            "http://p q",
            "SOCKS5://p",
            "http://user:pa^ss@h:8080",
            "http://${PROXY_HOST}:3128",
            "http:///x",
            "http://user@",
            "\"http://p:80\"",
            "/proxy",
        ] {
            assert_eq!(with(&[("HTTP_PROXY", bad)], http), None, "{bad}");
            let fallback = [("HTTP_PROXY", bad), ("all_proxy", "p")];
            assert_eq!(with(&fallback, http), Some("all_proxy"), "{bad}");
        }
    }

    #[test]
    fn doctor_names_a_proxy_it_did_not_use() {
        let env = |k: &str| match k {
            "HTTP_PROXY" => Some("http://proxy:3128".to_string()),
            _ => None,
        };
        let probe = Probe {
            env: &env,
            ollama_answers: &|_| true,
            on_path: &|_| false,
        };
        let (found, what) =
            keyless_status_with("ollama", &ProviderFlags::default(), &probe).unwrap();
        assert!(found);
        assert_eq!(
            what,
            "answers at http://127.0.0.1:11434; checked without the proxy in HTTP_PROXY, \
             which the provider uses unless NO_PROXY exempts this address"
        );
    }

    #[test]
    fn doctor_says_whether_a_keyless_provider_is_there() {
        use clap::Parser;
        // Ollama "answers" only at `answering`; the CLIs in `clis` are on PATH.
        let status = |name: &str,
                      active: (&str, Option<&str>),
                      answering: &str,
                      clis: &[&str],
                      host: Option<&str>| {
            let host = host.map(str::to_string);
            let env = move |k: &str| (k == "OLLAMA_HOST").then(|| host.clone()).flatten();
            let answering = answering.to_string();
            let ollama_answers = move |url: &str| url == answering;
            let clis: Vec<String> = clis.iter().map(|c| c.to_string()).collect();
            let on_path = move |p: &str| clis.iter().any(|c| c == p);
            let cli = Cli::try_parse_from(["kx"]).unwrap();
            let mut flags = flags_for(
                &cli,
                Choice {
                    name: active.0.to_string(),
                    base_url: None,
                    reason: None,
                },
            );
            flags.base_url = active.1.map(str::to_string);
            keyless_status_with(
                name,
                &flags,
                &Probe {
                    env: &env,
                    ollama_answers: &ollama_answers,
                    on_path: &on_path,
                },
            )
        };
        let local = "http://127.0.0.1:11434";
        let none = ("openai", None);
        assert_eq!(
            status("ollama", none, local, &[], None),
            Some((true, format!("answers at {local}")))
        );
        assert_eq!(
            status("ollama", none, local, &[], Some("box:1234")),
            Some((false, "nothing answers at http://box:1234".into()))
        );
        assert_eq!(
            status("ollama", none, local, &[], Some("   ")),
            Some((true, format!("answers at {local}")))
        );
        // The active provider's --base-url wins, as given, whatever the case
        // of the provider name; another provider's --base-url does not.
        let stub = "http://127.0.0.1:18734";
        assert_eq!(
            status(
                "ollama",
                ("Ollama", Some("http://127.0.0.1:18734/")),
                stub,
                &[],
                Some("box:1")
            ),
            Some((true, format!("answers at {stub}")))
        );
        assert_eq!(
            status(
                "ollama",
                ("ollama", Some("127.0.0.1:18734")),
                stub,
                &[],
                None
            ),
            Some((
                false,
                "nothing answers at an address that is not an http URL".into()
            ))
        );
        // Credentials in --base-url are sent but never shown.
        let with_password = "http://user:SECRET@127.0.0.1:18734/v1";
        assert_eq!(
            status(
                "ollama",
                ("ollama", Some(with_password)),
                with_password,
                &[],
                None
            ),
            Some((true, "answers at http://127.0.0.1:18734/v1".into()))
        );
        let not_http = "nothing answers at an address that is not an http URL";
        let ipv6 = "http://u:SECRET@[::1]:11434/?token=SECRET#SECRET";
        assert_eq!(
            status("ollama", ("ollama", Some(ipv6)), "", &[], None),
            Some((false, "nothing answers at http://[::1]:11434".into()))
        );
        // Where the password ends cannot be told, or the address is not one
        // the provider can use, so nothing of it is printed.
        for given in [
            "u:SECRET@box:1",
            "user:SECRET@box:1/?r=http://x",
            "http://user:SE/CRET@host:1",
            "ftp://box:1",
        ] {
            assert_eq!(
                status("ollama", ("ollama", Some(given)), "", &[], None),
                Some((false, not_http.into())),
                "{given}"
            );
        }
        assert_eq!(
            status("ollama", ("openai", Some(stub)), stub, &[], None),
            Some((false, format!("nothing answers at {local}")))
        );
        assert_eq!(
            status("claude-code", none, "", &["claude"], None),
            Some((true, "claude on PATH".into()))
        );
        assert_eq!(
            status("opencode", none, "", &["claude"], None),
            Some((false, "opencode not on PATH".into()))
        );
        assert_eq!(status("openai", none, local, &["claude"], None), None);
    }

    #[test]
    fn doctor_names_where_a_key_comes_from() {
        use clap::Parser;
        let cli = Cli::try_parse_from(["kx"]).unwrap();
        let named = |name: &str, key: Option<&str>| {
            let mut flags = flags_for(
                &cli,
                Choice {
                    name: name.to_string(),
                    base_url: None,
                    reason: None,
                },
            );
            flags.api_key = key.map(str::to_string);
            flags
        };
        let env = |k: &str| (k == "GROQ_API_KEY").then(|| "g".to_string());
        let source =
            |name: &str, var: &str, flags: &ProviderFlags| key_source(name, var, flags, &env);

        let flag_key = "--api-key or KERNEX_API_KEY".to_string();
        assert_eq!(
            source("openai", "OPENAI_API_KEY", &named("OpenAI", Some("k"))),
            Some(flag_key)
        );
        // The flags' key goes to the active provider only.
        assert_eq!(
            source("openai", "OPENAI_API_KEY", &named("groq", Some("k"))),
            None
        );
        assert_eq!(
            source("groq", "GROQ_API_KEY", &named("openai", Some("k"))),
            Some("GROQ_API_KEY".into())
        );
        assert_eq!(
            source("openai", "OPENAI_API_KEY", &named("openai", None)),
            None
        );
        let empty = |_: &str| Some(String::new());
        assert_eq!(
            key_source("groq", "GROQ_API_KEY", &named("openai", None), &empty),
            None
        );
    }
}
