# Catabase

Catabase is a desktop PostgreSQL query workspace built with Rust, Tauri 2, Svelte 5, Vite, and Monaco.

## Prerequisites

- Rust toolchain with Cargo
- Node.js 24+
- pnpm 10+
- Tauri CLI: `cargo install tauri-cli --version '^2'`
- Tauri platform prerequisites for your OS

## Dependency Policy

Use pnpm only. Do not use npm for this project.

`pnpm-workspace.yaml` enforces `minimumReleaseAge: 10080`, so pnpm refuses package versions published less than one week ago.

## Setup

```sh
pnpm install
```

## Run Desktop App

```sh
cargo tauri dev
```

Equivalent frontend-only dev server:

```sh
pnpm --dir ui dev
```

## Build

```sh
pnpm --dir ui build
cargo tauri build
```

## Verify

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir ui run check
pnpm --dir ui run build
```

## Current Scope

Implemented:

- Tauri desktop shell
- Rust core crate with SQLite persistence
- connection CRUD, URL import/sync, password fallback/keyring attempt
- PostgreSQL query execution and schema introspection
- SQL splitting and simple auto-limit pagination
- execution-target resolution from Monaco selection/cursor/current statement
- transaction banner plus backend transaction connection routing
- server-side query cancellation via PostgreSQL cancel tokens
- visible/full result export to clipboard or file
- inline edit preview flow with apply guard until table detection is available
- Svelte/Monaco UI shell with sidebar, tabs, editor, results, history, settings
- pnpm install age gate

In progress:

- source-table and primary-key detection for applying inline edits
- AI inline completion wiring
