# Browsing Available TTPs

List the loaded armory:

```sh
ran armory
ran armory --armory /path/to/TTPs
```

The table contains each TTP's ID, name, tactic, status, and description. It includes disabled design sketches so their status is visible; disabled TTPs are never applicable.

`ran armory` does not provide per-ID or tactic-filter flags. In `ran emulate`, the browser UI supports browsing by tactic and shows techniques applicable to the selected entity.

Status values are authoring metadata. `disabled` has runtime meaning: the applicability engine excludes that TTP. Sliver-only definitions currently use this status because no Rust Sliver backend exists.
