# Development setup

## Toolchain

Rust is installed **inside WSL (Ubuntu)**, not on the Windows side. As of
2026-09-10: rustc/cargo 1.98.1 (stable), installed via `rustup`, with a
working C linker (`/usr/bin/cc`) already present for linking. Windows-native
`cargo`/`rustc` are not installed and there's no current plan to install
them — do all building/testing through WSL.

The project files live on the Windows filesystem at
`C:\Users\Musa\Desktop\Ictus`, reachable from inside WSL at
`/mnt/c/Users/Musa/Desktop/Ictus`.

## Building

From a WSL shell:

```bash
cd /mnt/c/Users/Musa/Desktop/Ictus
source $HOME/.cargo/env   # only needed if cargo isn't already on PATH
cargo build
```

From a Windows terminal (PowerShell or Git Bash), invoke WSL directly
without opening an interactive shell first:

```
wsl.exe -d Ubuntu -- bash -lc 'cd /mnt/c/Users/Musa/Desktop/Ictus && cargo build'
```

## Workspace layout

- `crates/ictus-ir` — shared IR consumed by every frontend and the kernel.
- `crates/ictus-frontend-verilog` — Verilog parsing (`sv-parser`) → IR
  lowering.
- `crates/ictus-kernel` — the cycle-based execution engine.
- `crates/ictus-cli` — the `ictus` binary.

`Cargo.lock` is committed (this workspace produces a binary, `ictus-cli`),
per standard Rust practice for applications vs. libraries.

## Version control

GitHub remote and visibility: see repo settings / `git remote -v` for the
current source of truth rather than trusting this doc, since that can
change without a docs update. Commits in this repo append
`Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>` when authored
with Claude Code assistance.
