---
description: Build and install loop_codex binary
---

# Install loop_codex

This project is a FORK of codex. The upstream `codex` is installed via `brew install codex` and must NOT be overwritten. Our fork is installed as `loop_codex`.

**Do NOT run `cargo install --path cli`** — that overwrites the real `codex` binary.

## Steps

// turbo
1. Build the release binary:
```
cd /Users/edwardholmes/Documents/GitHub/AI_Things/Agent_loop_codex/codex-rs && cargo build --release
```

// turbo
2. Copy the binary as `loop_codex`:
```
cp /Users/edwardholmes/Documents/GitHub/AI_Things/Agent_loop_codex/codex-rs/target/release/codex ~/.cargo/bin/loop_codex
```

// turbo
3. Verify installation:
```
loop_codex --version
```
