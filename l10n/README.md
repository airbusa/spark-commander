# Translations

The app's words, one file per language, as in remoxio: `en.json` is the source
(each key's English `message` and a `description` for translators), the others
hold only `message`. English is every language's fallback. `locales.json` lists
the languages with their own names; `glossary.json` the names that stay as they
are and the terms to translate the same way everywhere.

## In the code

```rust
use sc_core::t;
t!("viewer.zoom.in")                              // "Zoom in (Ctrl++)"
t!("transfer.copy.title", name = name, to = dir)  // "Copy \"{name}\" to {to}"
t!("panel.files", count = n)                      // "{count, plural, one {# file} other {# files}}"
```

- Keys: `area.element[.purpose]`, lowercase, words joined by `_`; the area is
  the module or dialog (`viewer`, `editor`, `settings`, `transfer`, `sidebar`,
  `download`, `passwords`, …). A key's meaning never changes: a new meaning gets
  a new key, so an old translation cannot linger.
- Whole sentences with named placeholders, never pieces glued together (word
  order differs between languages). Numbers that choose a form are plurals:
  `{count, plural, one {# file} other {# files}}`.
- ICU syntax: a literal `{` or `}` is written `'{'`; `''` is an apostrophe. A
  single `'` elsewhere (French `l'élément`) is plain text.
- Not translated: log and debug output, file and folder names, keys' names
  (Ctrl+C), protocol and product names (see `glossary.json`), the system's own
  error texts (`{err}`), settings files' contents.
- The menus' words are made from the menu tables (`sc_core::menu`):
  `cargo test -p sc-core writes_menu_words -- --ignored` writes them into
  `en.json`; a test checks they match.

## Checks

`cargo test -p sc-core l10n` checks that every translation's keys are English's
with the same placeholders, and that every `t!("…")` in the code has its
English.
