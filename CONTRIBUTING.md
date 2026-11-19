# Contributing to SonicBoost

Thanks for your interest in improving SonicBoost! This guide covers how to get set up and how changes are reviewed.

## Getting started

1. Install the [prerequisites](README.md#%EF%B8%8F-building-from-source): Rust (MSVC), the VS 2022 C++ Build Tools and Node.js 20+.
2. Fork the repo and clone your fork.
3. Run the app:
   ```powershell
   npm install
   npm run dev
   ```
4. UI-only work doesn't need Rust at all. `npm run preview` opens the interface in a browser with a built-in demo engine (`ui/js/mock.js`).

## Project layout

| Area | Where |
|:--|:--|
| Audio engine, DSP | `src-tauri/src/audio.rs`, `dsp.rs`, `ring.rs`, `analyzer.rs` |
| Windows integration | `src-tauri/src/lib.rs` (tray, hotkeys, autostart), `media.rs` (Now Playing) |
| Interface | `ui/index.html`, `ui/style.css`, `ui/js/*` |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit together.

## Ground rules

- **Never allocate, lock or block inside an audio callback.** Use the atomics in `Params` and the lock-free `Ring` instead.
- **DSP changes need tests.** Add a test next to the code in `dsp.rs` and run:
  ```powershell
  cargo test --manifest-path src-tauri/Cargo.toml --lib
  ```
- **Keep the design system.** Monochrome only. Use the CSS variables (`--ink`, `--glass`, `--sh-*`), the radius scale (14 / 7 / 6 px) and soft layered shadows.
- **Motion** should be frame-rate independent: use `dt` and the `Spring` helper in `controls.js`.
- Format Rust with `cargo fmt` and check it with `cargo clippy`.

## Commit messages

Use short, imperative subjects with an optional scope:

```
feat(eq): add parametric mode
fix(audio): recover when the output device is removed
docs: clarify virtual cable setup
```

## Pull requests

1. Create a branch from `main`.
2. Keep each PR focused on one change. Include screenshots for UI changes.
3. Make sure CI passes. It runs the tests and builds the installers on Windows.
4. Add a line under **[Unreleased]** in `CHANGELOG.md`.

## Releasing (maintainers)

See [docs/RELEASING.md](docs/RELEASING.md).
