# Documentation style guide

This guide keeps the site consistent as more people edit it. A reviewer can reject a page by citing a numbered rule.

## 1. Language

All published documentation and user-facing strings are written in English. Spanish is allowed in source code comments and in personal notes under `docs/`, but anything a reader sees on the site is in English.

## 2. Voice and tense

Use the second person and the present tense. Write "You can change the theme" rather than "The user can change the theme" or "You will be able to change the theme".

## 3. Headings

Use sentence case for headings: "Getting started", not "Getting Started". Capitalize proper nouns normally.

## 4. Admonitions

Use `[!WARNING]`, `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]` and `[!CAUTION]` only for genuine warnings or genuinely important asides. Do not use them to make a paragraph stand out. If every page has one, none of them mean anything.

## 5. Product terminology

Use the exact product nouns:

- **pane** for a subdivision of a tab.
- **tab** for a top-level container in a window.
- **session** for the PTY and terminal state behind a pane or tab.
- **split** for the action of dividing a pane, or the resulting layout.

Do not use "window" when you mean "tab", or "terminal" when you mean "session".

## 6. No marketing adjectives

Avoid words like "fast", "powerful", "seamless", "modern" or "intuitive" in explanatory prose. State what a feature does and let the reader decide whether it is impressive.

## 7. Cite current sources

A published claim must trace to current source code under `src/`, to a documented command, or to behavior observed in a running build. Do not cite archived documents, ADRs, or earlier versions of this site as proof of current behavior.

## 8. One home for each fact

Defaults, full key lists, chord tables and theme palettes live in the generated reference pages. Prose pages name the relevant keys and link to the reference. Do not copy a default value into a feature page, because a copied default becomes a future lie.

## 9. Commit subjects that reach the changelog

`release-plz` copies the subjects of `feat`, `fix`, `perf`, `security` and the `(feat|fix)(packaging)` group verbatim into `CHANGELOG.md`, and from there onto the site. Write those commit subjects in English and in the imperative. Other commit types are skipped by `release-plz`, so their language is less important. There is no automated check for this; it is reviewed in pull request.
