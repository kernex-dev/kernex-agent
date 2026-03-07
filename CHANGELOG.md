# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Skills CLI commands (`kx skills list/add/remove/verify`)
- Skills permission model with trust levels (sandboxed, standard, trusted)
- SHA-256 integrity verification for installed skills
- Audit logging for skill operations
- CONTRIBUTING.md with development guidelines
- 35 new tests for skills module (54 total, up from 19)

## [0.1.0] - 2026-03-07

### Added

- Initial MVP — `kx dev` interactive coding assistant
- Conversation lifecycle with inline commands and one-shot mode
- Ctrl+C handler with graceful conversation close
- Multiline input support with `"""` delimiters
- Rustyline for readline support (history, line editing)
- `/facts` command to view and delete stored facts
- `.kx.toml` project config support
- `/search` command for FTS5 memory search
- Spinner indicator during LLM calls
- `/history` command for conversation history
- `/retry` command for failed completions
- `dev` as the default subcommand
- Claude CLI availability validation on startup
- Improved multiline prompt with line numbers
- `/config` command to show active configuration
