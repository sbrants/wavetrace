# Contributing to WaveTrace

Thank you for your interest in contributing! WaveTrace is a local-first desktop
companion for **The Tower** — window capture, OCR, SQLite storage, and a React
UI. This guide covers setup, testing, and how to submit changes.

By participating, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you start

- Read [README.md](README.md) for prerequisites and `npm run tauri dev`.
- Read [Goal.md](Goal.md) for product rules (parsing, skips vs jumps, game modes).
  Parser and state-machine changes should match the spec or update it in the same PR.
- **Windows** is the primary dev platform for OCR work. Linux and macOS builds use
  Tesseract; many unit tests run on all platforms, but the OCR regression corpus
  runs on Windows only.
- **macOS releases are currently broken** — tracked in [#4](https://github.com/sbrants/wavetrace/issues/4).
  macOS fixes welcome; coordinate on that issue before large bundling/OCR changes.

## Development setup

```powershell
git clone https://github.com/sbrants/wavetrace.git
cd wavetrace
npm install
npm run tauri dev
```

Debug builds show an **orange-bordered** icon and window title **WaveTrace (Dev)**.

### Useful commands

| Command | Purpose |
| ------- | ------- |
| `npm run lint` | ESLint (TypeScript + jsx-a11y) on `src/` |
| `npm run build` | Typecheck + Vite production build |
| `cd src-tauri; cargo test --release` | Rust unit tests (parser, DB, state machine, …) |
| `cd src-tauri; cargo test --release captured_corpus -- --nocapture` | OCR corpus report (Windows) |

Keep the repo on a **local drive** — `node_modules/` and `target/` do not survive
cloud-sync virtual filesystems (see README).

## What to work on

Look for issues labeled [**good first issue**](https://github.com/sbrants/wavetrace/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22)
or [**help wanted**](https://github.com/sbrants/wavetrace/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22).

Areas that often welcome contributions:

- **Parser / classifier** — coin suffixes, OCR typos, game-mode detection (`src-tauri/src/parser.rs`, `classify.rs`)
- **State machine** — wave skips, debounce, resume catch-up (`src-tauri/src/state_machine.rs`)
- **Frontend** — Dashboard, History, Settings (`src/`)
- **Accessibility** — see [docs/accessibility.md](docs/accessibility.md)
- **Docs** — README, Goal.md, CHANGELOG

If you plan a larger change, open an issue first so we can align on approach.

### Suggested starter tasks

Open issues labeled **good first issue**:

1. [#1 — Add Vitest + tests for `skipDisplay.ts`](https://github.com/sbrants/wavetrace/issues/1) — pure formatting logic in `src/skipDisplay.ts`; wire `npm test` into CI when done.
2. [#2 — Fix `react-hooks/exhaustive-deps` warnings in `History.tsx`](https://github.com/sbrants/wavetrace/issues/2) — two lint warnings around `compareRunIds.length` in `useEffect` deps (~lines 267, 283).
3. [#3 — Add a parser unit test for an OCR typo](https://github.com/sbrants/wavetrace/issues/3) — extend `src-tauri/src/parser.rs` tests (`ocr_quirks`, `live_ocr`, `wave_skip_ocr_typos`); no game capture required.

## Testing

### Rust

```powershell
cd src-tauri
cargo test --release
```

Add or update unit tests next to the code you change. Parser and state-machine
logic should have table-driven tests with clear case names.

### OCR regression corpus (Windows)

When OCR or classification behavior changes:

1. Re-run analysis on saved frames: `cargo run --example reanalyze_corpus`
2. If expectations changed intentionally, update labels via `cargo run --example label_corpus` or edit `fixtures/captured/manifest.json`
3. Run `cargo test --release captured_corpus -- --nocapture`

To capture new live frames (requires The Tower / emulator running):

```powershell
cargo run --example capture_fixtures -- --count 30 --label-detected
cargo run --example capture_fixtures -- --prune-misses
```

Reference PNGs at `fixtures/` root are covered by `fixtures/reference.json`.

### Frontend

```powershell
npm run lint
npm run build
```

There is no frontend test runner yet — manual UI checks in `npm run tauri dev` are
expected for UI changes. Pure logic in `src/*.ts` is a good place to add Vitest
later (see open **good first issue** tasks).

## Code style

- **Rust:** follow existing module layout; prefer small, focused functions; document
  non-obvious game/OCR rules with a link to Goal.md when helpful.
- **TypeScript / React:** functional components, typed Tauri API wrappers in
  `src/api.ts`; run `npm run lint` before pushing.
- **CHANGELOG:** add user-facing changes under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md)
  (Keep a Changelog format).

## Submitting a pull request

1. Fork the repo and create a branch from `main`.
2. Make focused commits; one logical change per PR when possible.
3. Ensure CI checks pass locally (see above).
4. Open a PR against `main` and fill out the PR template.
5. Link any related issue (`Fixes #123`).

Maintainers will review for correctness against Goal.md, test coverage, and UX.
You may be asked to extend fixtures or CHANGELOG entries.

### Branch protection

`main` requires a passing [CI workflow](.github/workflows/ci.yml) before merge. Open a pull request
(even for your own changes) so checks run. Do not merge with failing tests.

## Release / maintainer notes

These are not required for most contributors but document project hygiene:

- Version bumps: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `CHANGELOG.md`
- Tag `v*` pushes trigger the [Release workflow](.github/workflows/release.yml)
- Store packaging: [microsoft-store/README.md](microsoft-store/README.md)
- Accessibility manual checklist: [docs/accessibility.md](docs/accessibility.md#phase-e--process--release-planned)

## License

By contributing, you agree that your contributions will be licensed under the
[MIT License](LICENSE).
