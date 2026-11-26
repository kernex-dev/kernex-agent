//! Which of their own tools the agent CLIs (`claude-code`, `opencode`) may
//! use in `kx dev`, one-shot commands and pipelines.
//!
//! An agent CLI runs its tools in a child process. The OS sandbox keeps
//! that process to the project directory, but it cannot take the network
//! away: the CLI needs it to reach the model, and its shell inherits it,
//! along with the login token. So kx starts the CLI with a tool list
//! instead of with its permission checks off. The default list reads,
//! searches and edits files and has no shell, so unless `web_fetch = "all"`
//! adds the CLI's `WebFetch` and `WebSearch`, no tool the model runs can
//! send data out.
//!
//! The user widens it in `~/.kx/config.toml` (`cli_tools = "all"` or a list
//! such as `["Read", "Edit", "Bash(cargo test *)"]`) or for one run with
//! `--cli-tools`. A project's `.kx.toml` cannot set it. `kx serve` uses its
//! own `--tools` profile instead.

use kernex_providers::WebFetchPolicy;
use serde::Deserialize;

use crate::agent_sandbox::{AccessArgs, UserConfig};
use crate::runtime_glue::ProviderFlags;

/// What the CLI may use when nothing is configured: read, search and edit
/// files, and keep a task list. No shell and no network.
pub const DEFAULT_CLI_TOOLS: &[&str] = &[
    "Read",
    "Glob",
    "Grep",
    "Edit",
    "Write",
    "NotebookEdit",
    "TodoWrite",
];

/// `cli_tools` in `~/.kx/config.toml`: `"all"`, `"default"`, or a list of
/// tools.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum CliToolsSetting {
    Keyword(String),
    Tools(Vec<String>),
}

/// Built-in tools of the Claude CLI a list may name. Any other entry must
/// be an MCP rule (`mcp__server__*`).
const KNOWN_TOOLS: &[&str] = &[
    "Bash",
    "Read",
    "Write",
    "Edit",
    "MultiEdit",
    "Grep",
    "Glob",
    "LS",
    "NotebookEdit",
    "Task",
    "TodoWrite",
    "WebFetch",
    "WebSearch",
];

const CONFIG: &str = "~/.kx/config.toml";

/// Where `web_fetch` for a run comes from, for messages.
const WEB_FETCH_SOURCES: &str =
    "it comes from --web-fetch, else a project's web_fetch = \"none\", else ~/.kx/config.toml";

/// Whether `provider` is an agent CLI that runs its own tools. Case is
/// ignored, as the provider factory ignores it.
pub fn is_agent_cli(provider: &str) -> bool {
    ["claude-code", "opencode"]
        .iter()
        .any(|cli| provider.trim().eq_ignore_ascii_case(cli))
}

/// The tool list for `provider`: [`resolve`] for an agent CLI, `None` for
/// a provider whose tools kx runs itself.
pub fn for_provider(
    provider: &str,
    args: &AccessArgs,
    user: &UserConfig,
    web_fetch: &WebFetchPolicy,
) -> anyhow::Result<Option<Vec<String>>> {
    if !is_agent_cli(provider) {
        return Ok(None);
    }
    resolve(&args.cli_tools, user.cli_tools.as_ref(), web_fetch)
}

/// The tool list `build_provider` gives the agent CLI of this run, `None`
/// for every tool or for a provider that runs no tools of its own. Only
/// whether it names `Read` matters to callers, so `web_fetch` is not read.
pub fn for_flags(flags: &ProviderFlags) -> anyhow::Result<Option<Vec<String>>> {
    for_provider(
        &flags.name,
        &flags.access,
        &UserConfig::load()?,
        &WebFetchPolicy::Disabled,
    )
}

/// The tool list for an agent CLI: `--cli-tools` wins, then the user's
/// config, then [`DEFAULT_CLI_TOOLS`]. `None` means every tool, with the
/// CLI's permission checks off. The default list gets `WebFetch` and
/// `WebSearch` when `web_fetch` allows every host; a list the user wrote
/// is passed as written. A list is never empty: the CLI cannot be started
/// with no tools.
pub fn resolve(
    flag: &[String],
    user: Option<&CliToolsSetting>,
    web_fetch: &WebFetchPolicy,
) -> anyhow::Result<Option<Vec<String>>> {
    let from_flag: Vec<&str> = flag.iter().flat_map(|v| split_rules(v)).collect();
    let chosen = if !from_flag.is_empty() {
        parse(&from_flag, "--cli-tools")?
    } else {
        match user {
            None => Choice::Default,
            Some(CliToolsSetting::Keyword(k)) => match keyword(k.trim(), CONFIG)? {
                Some(choice) => choice,
                None => anyhow::bail!(
                    "{CONFIG}: cli_tools = {k:?} is not valid; use \"all\", \"default\" or \
                     a list of tools such as [\"Read\", \"Edit\"]"
                ),
            },
            Some(CliToolsSetting::Tools(list)) => {
                let list: Vec<&str> = list.iter().map(|t| t.trim()).collect();
                Choice::List(checked(&list, CONFIG)?)
            }
        }
    };
    Ok(match chosen {
        Choice::All => None,
        Choice::List(list) => Some(list),
        Choice::Default => {
            let mut list: Vec<String> = DEFAULT_CLI_TOOLS.iter().map(|t| t.to_string()).collect();
            if matches!(web_fetch, WebFetchPolicy::AllowAll) {
                list.extend(["WebFetch".to_string(), "WebSearch".to_string()]);
            }
            Some(list)
        }
    })
}

/// `list` without the fetch tools `web_fetch` keeps off this run (by the
/// flag, the user's config or a project's `web_fetch = "none"`): the CLI
/// gets them only when every host is allowed. kx warns when it takes one
/// out, and refuses when nothing would be left: a CLI started with an empty
/// list could use no tool at all.
pub fn without_blocked_fetch(
    list: Vec<String>,
    web_fetch: &WebFetchPolicy,
) -> anyhow::Result<Vec<String>> {
    if matches!(web_fetch, WebFetchPolicy::AllowAll) {
        return Ok(list);
    }
    let is_fetch = |t: &String| matches!(fold(rule_name(t)).as_str(), "webfetch" | "websearch");
    let (dropped, kept): (Vec<String>, Vec<String>) = list.into_iter().partition(is_fetch);
    if dropped.is_empty() {
        return Ok(kept);
    }
    if kept.is_empty() {
        anyhow::bail!(
            "the CLI tool list names only {}, which the CLI gets only while web_fetch is \
             \"all\", and it is not for this run ({WEB_FETCH_SOURCES}). Set it to \"all\" or \
             add another tool to the list",
            dropped.join(" and ")
        );
    }
    eprintln!(
        "warning: the CLI tool list names {}, left out because web_fetch is not \"all\" for \
         this run ({WEB_FETCH_SOURCES})",
        dropped.join(" and ")
    );
    Ok(kept)
}

enum Choice {
    All,
    Default,
    List(Vec<String>),
}

/// A flag value split into rules at the commas outside parentheses, so
/// `Read,Bash(echo a,b)` gives two rules.
fn split_rules(value: &str) -> Vec<&str> {
    let mut rules = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (i, c) in value.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                rules.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    rules.push(&value[start..]);
    rules
        .into_iter()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .collect()
}

fn parse(values: &[&str], origin: &str) -> anyhow::Result<Choice> {
    if let [single] = values {
        if let Some(choice) = keyword(single, origin)? {
            return Ok(choice);
        }
    }
    Ok(Choice::List(checked(values, origin)?))
}

/// The choice `value` names when it is a keyword (any case), `None` when
/// it is not one. `none` is refused: a CLI with no tools can only talk, and
/// an API provider does that without starting one.
fn keyword(value: &str, origin: &str) -> anyhow::Result<Option<Choice>> {
    match value.to_ascii_lowercase().as_str() {
        "all" => Ok(Some(Choice::All)),
        "default" => Ok(Some(Choice::Default)),
        "none" => anyhow::bail!(
            "{origin}: the agent CLIs cannot run with no tools; use an API provider such as \
             `anthropic` for that, or a short list such as [\"Read\"]"
        ),
        _ => Ok(None),
    }
}

/// `name` folded for comparison: lowercase, underscores dropped.
fn fold(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_')
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The tool name of a rule (`Bash(git *)` gives `Bash`).
fn rule_name(rule: &str) -> &str {
    rule.split('(').next().unwrap_or(rule).trim()
}

/// Whether the CLI reads `rule` as the one rule it looks like: a name of
/// ASCII letters, digits, `_`, `-` and `*`, then at most one parenthesized
/// pattern that ends the rule. kernex drops any other entry.
fn is_single_rule(rule: &str) -> bool {
    let (name, suffix) = match rule.find('(') {
        Some(i) => (&rule[..i], &rule[i..]),
        None => (rule, ""),
    };
    let name_ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '*'));
    let suffix_ok = suffix.is_empty()
        || (suffix.len() >= 2
            && suffix.ends_with(')')
            && !suffix[1..suffix.len() - 1].contains(['(', ')']));
    name_ok && suffix_ok
}

/// `tools` as a list of known tools or MCP rules, without keywords, empty
/// entries or repeats. An empty list is refused (see [`keyword`]).
fn checked(tools: &[&str], origin: &str) -> anyhow::Result<Vec<String>> {
    if tools.is_empty() {
        keyword("none", origin)?;
    }
    let mut list: Vec<String> = Vec::new();
    for tool in tools {
        if tool.is_empty() {
            anyhow::bail!("{origin}: a tool name is empty");
        }
        if matches!(
            tool.to_ascii_lowercase().as_str(),
            "all" | "default" | "none"
        ) {
            anyhow::bail!("{origin}: `{tool}` cannot be combined with tool names");
        }
        if !is_single_rule(tool) {
            anyhow::bail!(
                "{origin}: `{tool}` is not one rule; write a tool name with at most one \
                 pattern in parentheses at its end, such as Bash(cargo test *)"
            );
        }
        let name = rule_name(tool);
        let known = KNOWN_TOOLS.iter().any(|k| fold(k) == fold(name));
        if !known && !name.starts_with("mcp__") {
            anyhow::bail!(
                "{origin}: `{tool}` is not a Claude CLI tool; use {} or an MCP rule such as \
                 mcp__server__*",
                KNOWN_TOOLS.join(", ")
            );
        }
        if !list.iter().any(|t| t == tool) {
            list.push(tool.to_string());
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flag(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn default_list() -> Vec<String> {
        DEFAULT_CLI_TOOLS.iter().map(|t| t.to_string()).collect()
    }

    fn off() -> WebFetchPolicy {
        WebFetchPolicy::Disabled
    }

    fn keyword_setting(k: &str) -> CliToolsSetting {
        CliToolsSetting::Keyword(k.into())
    }

    #[test]
    fn nothing_set_gives_the_default_list_without_a_shell_or_fetch() {
        let list = resolve(&[], None, &off()).unwrap().unwrap();
        assert_eq!(list, default_list());
        for missing in ["Bash", "WebFetch", "WebSearch", "Task"] {
            assert!(!list.iter().any(|t| t == missing), "{missing} is offered");
        }
        assert!(
            list.iter().any(|t| t == "Read"),
            "skills need the read tool"
        );
    }

    #[test]
    fn the_default_list_gets_the_fetch_tools_only_when_every_host_is_allowed() {
        let all = resolve(&[], None, &WebFetchPolicy::AllowAll)
            .unwrap()
            .unwrap();
        assert!(all.iter().any(|t| t == "WebFetch"));
        assert!(all.iter().any(|t| t == "WebSearch"));
        let hosts = WebFetchPolicy::AllowHosts(vec!["docs.rs".into()]);
        assert_eq!(resolve(&[], None, &hosts).unwrap().unwrap(), default_list());
    }

    #[test]
    fn the_user_config_sets_all_default_or_a_list() {
        assert_eq!(
            resolve(&[], Some(&keyword_setting("all")), &off()).unwrap(),
            None
        );
        assert_eq!(
            resolve(&[], Some(&keyword_setting("ALL")), &off()).unwrap(),
            None
        );
        assert_eq!(
            resolve(&[], Some(&keyword_setting("default")), &off()).unwrap(),
            Some(default_list())
        );
        let list = CliToolsSetting::Tools(flag(&["Read", "Bash(cargo test *)", "Read"]));
        assert_eq!(
            resolve(&[], Some(&list), &WebFetchPolicy::AllowAll).unwrap(),
            Some(flag(&["Read", "Bash(cargo test *)"])),
            "a written list is passed as written, without the fetch tools"
        );
        for bad in ["everything", "Read"] {
            assert!(
                resolve(&[], Some(&keyword_setting(bad)), &off()).is_err(),
                "{bad}"
            );
        }
    }

    /// kernex 0.12 read an empty tool list as no list, which offered every
    /// tool. 0.13 keeps it as no tools, and kx still refuses "no tools"
    /// rather than start a CLI that can do nothing.
    #[test]
    fn no_tools_is_refused_rather_than_widened() {
        let none = keyword_setting("none");
        assert!(resolve(&[], Some(&none), &off()).is_err());
        let empty = CliToolsSetting::Tools(Vec::new());
        assert!(resolve(&[], Some(&empty), &off()).is_err());
        assert!(resolve(&flag(&["none"]), None, &off()).is_err());
        assert!(resolve(&flag(&["None"]), None, &off()).is_err());
    }

    #[test]
    fn the_flag_wins_over_the_user_config() {
        let all = keyword_setting("all");
        assert_eq!(
            resolve(&flag(&["default"]), Some(&all), &off()).unwrap(),
            Some(default_list())
        );
        assert_eq!(
            resolve(&flag(&["Read,Grep"]), Some(&all), &off()).unwrap(),
            Some(flag(&["Read", "Grep"]))
        );
        assert_eq!(resolve(&flag(&["All"]), None, &off()).unwrap(), None);
        assert_eq!(
            resolve(&flag(&["  ", ","]), Some(&all), &off()).unwrap(),
            None,
            "an empty flag leaves the config in charge"
        );
    }

    #[test]
    fn commas_inside_a_pattern_stay_in_the_rule() {
        assert_eq!(
            resolve(&flag(&["Read, Bash(echo a,b)"]), None, &off()).unwrap(),
            Some(flag(&["Read", "Bash(echo a,b)"]))
        );
        assert_eq!(
            resolve(&flag(&["Read", "Edit"]), None, &off()).unwrap(),
            Some(flag(&["Read", "Edit"])),
            "the flag can also be repeated"
        );
    }

    #[test]
    fn a_list_takes_known_tools_and_mcp_rules_only() {
        assert_eq!(
            resolve(&flag(&["read"]), None, &off()).unwrap(),
            Some(flag(&["read"])),
            "a single tool name reads like a longer list"
        );
        assert_eq!(
            resolve(&flag(&["read,web_fetch,mcp__github__*"]), None, &off()).unwrap(),
            Some(flag(&["read", "web_fetch", "mcp__github__*"]))
        );
        for bad in [
            "bogus",
            "Read,Shell",
            "Edit(x),Bashful",
            "Bash (curl *)",
            "Bash(cargo *,Read",
            "Bash(cargo test)(x)",
            "Bash(a)b",
        ] {
            assert!(resolve(&flag(&[bad]), None, &off()).is_err(), "{bad}");
        }
        assert!(resolve(&flag(&["all,Read"]), None, &off()).is_err());
        let mixed = CliToolsSetting::Tools(flag(&["Read", "none"]));
        assert!(resolve(&[], Some(&mixed), &off()).is_err());
        let empty = CliToolsSetting::Tools(flag(&["Read", " "]));
        assert!(resolve(&[], Some(&empty), &off()).is_err());
    }

    #[test]
    fn a_fetch_tool_in_a_list_needs_every_host_allowed() {
        let list = flag(&["Read", "web_fetch", "WebSearch"]);
        assert_eq!(
            without_blocked_fetch(list.clone(), &off()).unwrap(),
            flag(&["Read"])
        );
        let hosts = WebFetchPolicy::AllowHosts(vec!["docs.rs".into()]);
        assert_eq!(
            without_blocked_fetch(list.clone(), &hosts).unwrap(),
            flag(&["Read"])
        );
        assert_eq!(
            without_blocked_fetch(list, &WebFetchPolicy::AllowAll).unwrap(),
            flag(&["Read", "web_fetch", "WebSearch"])
        );
        assert_eq!(
            without_blocked_fetch(default_list(), &off()).unwrap(),
            default_list()
        );
        // Nothing left: the CLI would start with no tool at all.
        assert!(without_blocked_fetch(flag(&["WebFetch"]), &off()).is_err());
    }

    #[test]
    fn only_the_agent_clis_take_the_list() {
        assert!(is_agent_cli("claude-code"));
        assert!(is_agent_cli("Claude-Code"), "the factory ignores case");
        assert!(is_agent_cli("OPENCODE"));
        assert!(
            is_agent_cli(" claude-code "),
            "the provider builder trims the name, so this must too"
        );
        assert!(!is_agent_cli("anthropic"));
        let user = UserConfig {
            cli_tools: Some(keyword_setting("bogus")),
            ..Default::default()
        };
        let args = AccessArgs::default();
        assert_eq!(
            for_provider("anthropic", &args, &user, &off()).unwrap(),
            None
        );
        assert!(for_provider("claude-code", &args, &user, &off()).is_err());
    }
}
