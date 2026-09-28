# Agent Instructions

- Don't use any EM-dashes!

## Repository Notes

- The examples directory contains testing and playground material only.
- Treat files under examples as non-critical by default.
- Ignore examples content during analysis, triage, and refactors unless the user explicitly asks to work in examples.
- If a change request spans the repo, prioritize all other directories first and include examples only when specifically requested.
- Keep `armory/vocabulary.json` synchronized with every added, changed, or removed Armory procedure field, `requires` key, and `effects` kind.
- Regenerate `docs/book/src/reference/armory-vocabulary.md` with `cargo run -p armory --bin generate-vocabulary-docs` and run `cargo test -p armory --locked` after vocabulary changes. Do not edit the generated page directly.
