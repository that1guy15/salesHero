# Developer setup

What a new machine needs, and why. Nothing here is required to *build* — the
workspace builds with a stock Rust toolchain — but the project's own handoff
notes ask for most of it, and the reasons are real.

---

## Required

```bash
rustup toolchain install stable          # workspace needs >= 1.82
rustup component add rustfmt clippy rust-analyzer
```

**`rust-analyzer` is a component, not a separate download.** The
`rust-analyzer-lsp` Claude Code plugin expects the binary on PATH; with the
plugin enabled and the component missing it silently does nothing.

```bash
cargo build --workspace && cargo test --workspace
```

No audio hardware, network, API key, or model download is needed for any of it.

---

## Strongly recommended

### `sccache` — shared compilation cache

```bash
cargo install --locked sccache
export RUSTC_WRAPPER=sccache          # add to your shell profile
```

`docs/PROJECT-STATUS.md` §4.5 asks for this by name. The dependency graph is
heavy — `ort`, `fastembed`, `tauri`, bundled SQLite — and parallel agents working
in separate git worktrees each get their own `target/`, rebuilding all of it.
A shared `CARGO_TARGET_DIR` is the alternative.

Deliberately **not** set in a committed `.cargo/config.toml`: CI does not have
sccache, and a wrapper that is not installed is a build failure rather than a
slow build.

### `cargo-nextest` — faster test runner

```bash
cargo install --locked cargo-nextest   # needs rustc >= 1.91; use a newer
                                       # toolchain just for the install
cargo nextest run --workspace
```

### `cargo-deny` — licence and advisory checks

```bash
cargo install --locked cargo-deny
cargo deny check
```

This repo is public, MIT, and pulls a large dependency tree. `PROJECT-STATUS.md`
§4.3 asks for supply-chain hygiene before more contributors arrive.

---

### `gitleaks` — secret scanning

```bash
brew install gitleaks          # or: https://github.com/gitleaks/gitleaks/releases
make secret-scan
```

CI enforces this over full history. Running it locally means finding a mistake
before it is in a commit that has to be rewritten.

---

## The inner loop

`make` is the canonical surface. CI runs these same targets, so "it passed
locally" and "it passed in CI" mean the same thing.

```bash
make doctor       # what this machine is missing
make gate         # fmt, clippy on both feature sets, tests, detection gate
make audit        # advisories, licences, banned crates, sources
make secret-scan
make calibrate EMBEDDER=hybrid   # the M1.5 report
make overlay      # run the app
```

While working, `cargo check --workspace` is the fast loop. `make gate` before
every push.

**`cargo fmt --all` before every commit.** CI's first step is
`cargo fmt --all -- --check` with `RUSTFLAGS: -D warnings`, and a formatting
diff fails the build. This has been red on `main` before. The project's
`.claude/settings.json` runs `cargo fmt` automatically after Claude Code edits a
`.rs` file; if you edit by hand, run it yourself.

---

## Windows

`crates/audio` (M4), the overlay's capture exclusion (M6) and the installer (M9)
cannot be built or verified on macOS or Linux. They need a real Windows 11
machine with audio hardware and the **MSVC** toolchain — `x86_64-pc-windows-gnu`
is not what Tauri and WASAPI build against, and CI's `windows-latest` runner
proves compilation but not audio behaviour.

The manual checklists are in the `windows-verify` skill.

---

## Model weights

Never committed. `models/`, `*.onnx`, `*.gguf` and `*.safetensors` are
gitignored — Parakeet TDT alone is 631–680 MB and these carry their own licences.

The default embedder needs no model at all. The real semantic gate downloads
BGE-small on first use:

```bash
cargo run -p riposte-cli --features onnx -- --onnx detect --text "we're all set thanks" --all
```

Read `docs/PROJECT-STATUS.md` §3 before trusting either one's output.

---

## A note on call audio

`.gitignore` excludes `**/*.wav` and friends with a narrow allowance for
synthetic fixtures. This is **not** hygiene. Real call audio in a public repo is
a criminal exposure in two-party-consent states. Synthetic TTS fixtures only,
and keep real recordings local and gitignored.
