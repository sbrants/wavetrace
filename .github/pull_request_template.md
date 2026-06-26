## Summary

<!-- What does this PR change and why? Link Goal.md sections if behavior changes. -->

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Refactor / chore
- [ ] Documentation
- [ ] OCR / fixture update

## Test plan

- [ ] `npm run lint` and `npm run build`
- [ ] `cd src-tauri; cargo test --release`
- [ ] OCR corpus (Windows only, if parser/classify/OCR changed): `cargo test --release captured_corpus -- --nocapture`
- [ ] Manual UI check in `npm run tauri dev` (if frontend changed)

## Changelog

- [ ] Added an entry under `[Unreleased]` in CHANGELOG.md (user-facing changes only)

## Screenshots / logs

<!-- Optional: UI before/after, corpus report output, etc. -->
