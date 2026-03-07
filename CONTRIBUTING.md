# Contributing to kx

Thank you for your interest in contributing to kx. This document outlines the process for contributing to the project.

## Before You Start

Read [CLAUDE.md](CLAUDE.md) for the full project principles and code standards.

## Development Setup

1. Clone the repository:

```bash
git clone https://github.com/kernex-dev/kernex-agent.git
cd kernex-agent
```

2. Build:

```bash
cargo build
```

3. Run the test suite:

```bash
cargo test
```

## Pre-Commit Checklist

All checks must pass before committing:

| Step | Command |
|------|---------|
| 1 | `cargo build` |
| 2 | `cargo audit` |
| 3 | `cargo clippy -- -D warnings` |
| 4 | `cargo test` |
| 5 | `cargo fmt --check` |

Only commit after all five steps pass.

## Commit Style

Use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — new feature
- `fix:` — bug fix
- `refactor:` — code restructuring without behavior change
- `docs:` — documentation only
- `test:` — adding or updating tests
- `chore:` — maintenance tasks (deps, config, CI)

Keep commits atomic: one logical change per commit.

## Pull Request Process

1. Fork the repository and create a feature branch:

```bash
git checkout -b feat/my-feature
```

2. Make your changes and run the full pre-commit gate.

3. Push to your fork and open a Pull Request against `main`.

4. Ensure CI passes. Address any review feedback.

## Code Standards Summary

- **No `unwrap()` or `expect()`** in production code. Use `?` and proper error types.
- **File size:** Keep files under 500 lines (excluding tests).
- **Logging:** Use `tracing` or `eprintln!` for CLI feedback.
- **Async:** All I/O is async via Tokio.

## Relationship to Kernex

kx is a thin CLI wrapper around [kernex-runtime](https://github.com/kernex-dev/kernex). For changes to the core runtime, providers, or memory system, contribute to the main Kernex repository instead.

## License

By contributing, you agree that your contributions will be licensed under the same terms as the project: Apache-2.0 OR MIT.
