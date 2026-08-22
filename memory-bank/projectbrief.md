# Project Brief — Agency Agents

**Product**: Agency Agents
**Bundle ID**: `com.zerologic.agency-agents-app`
**Repo**: github:msitarzewski/agency-agents-app
**Author**: Michael Sitarzewski / Zerologic
**License**: MIT · No telemetry · No accounts

## One-liner

A native macOS app store for AI agents. Browse, search, install, and **track** the
**210** specialized agent personas (across 16 categories) from
[`msitarzewski/agency-agents`](https://github.com/msitarzewski/agency-agents)
across every AI coding tool you use (Claude Code, Cursor, Codex, Gemini CLI, Copilot,
Windsurf, Aider, opencode, qwen, openclaw, antigravity).
(The repo also ships NEXUS playbooks + workflow examples — docs, not agents — a candidate
for a future "Playbooks" section.)

## Thesis

This app is to the **agency-agents** repo what **brew-browser** is to **Homebrew**:
a thin, respectful native GUI over a CLI/content catalog. We forked brew-browser
structurally (Tauri 2 + Svelte 5 + Rust) and re-pointed the domain layer from
`brew` packages to AI agents.

**Key difference from brew-browser**: Homebrew owns its own install database
(`brew list`/`brew outdated` are the source of truth). The AI tools have **no such
registry** — every tool's config dir is a dumb dumping ground. So *we must BE the
database*. That state-tracking layer is the app's core differentiator:
> "the only tool that tracks your agents across every AI tool you use."

## What it is / isn't

- **Is**: a browser + installer + cross-tool state tracker + loadout manager for agents.
- **Isn't**: a replacement for the agency-agents repo, telemetry-funded, freemium,
  or a hosted multi-tenant agency platform.

## IntentOS Runtime v0.1 branch extension

The `intentos/runtime-v0.1` branch adds an experimental, local execution surface on
top of the existing catalog and project registry. It runs a fixed five-stage pipeline
through an installed Codex CLI and persists run evidence locally. This extension does
not change the upstream Agency Agents release version (`0.3.0`) and is not part of the
published app until it is separately reviewed and merged.

Runtime v0.1 is deliberately narrow: one provider, sequential stages, one project
directory, catalog-backed personas, explicit PASS/FAIL markers, one QA remediation
loop capped at three attempts, cancellation, resumption of passed stages, and local
JSON evidence. It is not NeMo, a model router, a parallel agent scheduler, a human
approval system, a cost controller, or a transactional rollback engine.

## Source of truth (the corpus-copy model)

The app maintains its **own local working copy** of the agency-agents repo in
`~/Library/Application Support/com.zerologic.agency-agents-app/corpus/`, seeded from a
bundled baseline and refreshed from the GitHub tarball. From that copy it derives a
hash index (`corpus-index.json`) and renders/install agents into each tool. See
`systemPatterns.md`.

## Inherited wholesale from brew-browser (do not redesign)

Tauri 2 shell · Svelte 5 runes frontend · streaming job model
(`run_*_streaming` → `Channel<Event>` + global write-lock) · design system
(`src/lib/styles/tokens.css`, components) · command palette · settings shell ·
window chrome (overlay titlebar + vibrancy) · minisign in-app updater ·
`sign-and-notarize.sh` / `publish-manifest.sh` · AGENTS.md methodology + memory-bank.

## Status

Phase 0 (fork + rebrand) — DONE. See `phases/phase-roadmap.md` and `activeContext.md`.
