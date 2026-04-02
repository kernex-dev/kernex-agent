# The kx Manifesto

## What Problem Does kx Solve?

You work across multiple projects: different stacks, different conventions, different states of documentation. Every time you context-switch, you lose the thread of what was decided, what patterns the codebase uses, what that last error actually meant.

AI coding tools help with individual questions. They do not remember. They do not know your project. They start fresh every session.

kx is different. It is a project-aware coding assistant that runs in your terminal, remembers context across sessions, and builds understanding of your project the longer you use it.

---

## kx vs the Tools You Already Know

### vs Claude Code, Cursor, Copilot

These are excellent tools. They autocomplete code, suggest edits, explain errors inline. kx does not compete at that layer.

kx is a reasoning partner. It operates at the level of questions: why is this architecture failing, what is the right approach for this problem, what did we decide about this pattern three weeks ago. It runs outside your IDE and works with any editor or no editor at all.

The key difference is memory. Every session adds to a per-project knowledge base. Facts you store, decisions you record, patterns the assistant learns from your corrections: all persist. The next session starts with context, not a blank slate.

### vs Aider

Aider applies patches. You describe a change, it writes the code. That is valuable for mechanical tasks.

kx operates at a higher level. It is a thinking tool, not a patch tool. You stay in control of what gets written. kx explains, suggests, reasons. You decide what to apply. For complex architectural decisions, a tool that moves fast and applies patches can create more work than it saves. kx is for the conversations that happen before you write the code.

### vs shell-gpt and generic CLI tools

Generic CLI AI tools answer questions about anything from a blank context. kx knows what project you are in.

When you run kx inside a Rust project, it detects the stack, loads the relevant skills, and starts with context from your previous sessions in that project. The same binary in a Python project behaves like a Python assistant. Stack awareness is not a prompt trick. It is wired into the runtime.

### vs GitHub Copilot CLI

Copilot CLI suggests shell commands. kx holds a conversation about your project: reasoning through architecture, reviewing decisions, surfacing what was learned in previous sessions. The scope is broader and the memory is persistent.

---

## The Memory Difference

Most AI tools treat each session as independent. kx maintains a per-project SQLite database with:

- **Conversation history** for contextual recall across sessions
- **Facts** via `/facts`: explicit key-value knowledge you store and retrieve
- **Lessons**: patterns learned from corrections across your sessions
- **Rewards**: reinforcement signals that surface useful lessons in future context

This is not a gimmick. It is the difference between a tool you use once and a tool that becomes more useful the longer you work in a project.

---

## Security by Default

kx runs every AI subprocess inside an OS-level sandbox. On macOS, Seatbelt. On Linux, Landlock. The AI cannot read files outside the project directory, write to system paths, or make unexpected network calls.

You do not have to trust the model. The OS enforces the boundary.

The skills system adds a second layer. Skills are text-only `SKILL.md` files: no scripts, no binaries. Every installed skill is SHA-256 hashed. `kx skills verify` detects tampering. A permission model lets you control what skills can influence: `context:files`, `suggest:edits`, `suggest:commands`. You decide what each skill is allowed to do.

---

## Who Uses kx

### The Solo Developer

You maintain five to fifteen projects across different stacks. You cannot keep all the context in your head and the documentation is always a few pivots behind. kx acts as an external brain for each project: you ask questions, it answers with context from previous sessions, you record decisions so the next session picks up where you left off.

No API key required. kx defaults to Claude Code CLI, which runs on a Claude Max subscription with no per-token billing.

### The Team

Skills are text files. Any team can ship a `SKILL.md` that encodes project conventions: naming rules, error handling patterns, architecture decisions, the things that live in someone's head and get lost when they leave. Commit it to the repo. Every developer gets the same context without reading documentation.

kx supports multiple AI providers. Team members on Anthropic API, others using Ollama offline, some on OpenRouter: the same tool with different backends.

### The Developer Who Cares About Reproducibility

Every skill install is verified. Every operation is audit-logged. Every provider can be pinned to a specific model. The configuration lives in `.kx.toml` in your project root, versioned alongside your code.

What kx does in your project is transparent, inspectable, and reproducible.

---

## What kx Does Not Do

kx does not modify your files. It suggests changes. You apply them.

This is a choice, not a limitation. For architecture decisions, debugging complex issues, and reasoning about tradeoffs, the bottleneck is judgment rather than typing speed. kx helps with the judgment. You provide the oversight.

If you want a tool that writes and applies code autonomously, kx is not that tool. It is a tool that makes you faster at writing and deciding yourself.

---

## Built on Kernex

kx is a thin CLI wrapper around the [Kernex](https://github.com/kernex-dev/kernex) runtime: the same open-source Rust framework that any developer can use to build custom agent applications.

Everything kx does is replicable and extensible. The memory system, skill loader, provider backends, sandbox: all available as composable Rust crates on crates.io.

If you need something kx does not do, build it. The framework is open and the primitives are composable.

---

## The Principle

> A good tool knows what project you are in, remembers what you decided, and stays out of your way.

kx is a terminal-native, memory-persistent, stack-aware coding assistant. It is not trying to replace your editor or automate your judgment. It is trying to make the hours you spend thinking about code more effective.

Start with `kx init`. Then just run `kx`.
