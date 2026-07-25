# Keybindings

## Overriding a binding

Add entries to the `[keys]` table, mapping a chord string to an action name:

```toml
[keys]
"ctrl+shift+t" = "new_tab"
"ctrl+alt+right" = "focus_next_pane"
```

A chord is written as zero or more modifiers joined with `+`, in any order, followed by the key: `ctrl`/`control`, `alt`/`meta`, `shift`, `super`/`cmd`. The key itself is a single character (`t`, `=`, `[`), a named key (`up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `insert`, `delete`, `enter`, `tab`, `escape`/`esc`, `backspace`), or a function key (`f1`–`f12`).

An override replaces whatever the default binding for that exact chord was; it does not merge with or chain to it. See the [keybindings reference](../reference/keybindings.md) for the full default chord table, including the action string to use on the right-hand side.

## Invalid entries

If a chord or action name in `[keys]` fails to parse, that single entry is skipped — it does not fail config loading, and every other override still applies. The skipped entry is only reported as a warning-level log line (visible with `RUST_LOG=baud=warn` or higher; see [Troubleshooting](../help/troubleshooting.md)), not as an on-screen message, so a typo in an override is silent unless you have logging on.

## Platform-specific chords

Almost every default binding is identical on Linux and Windows. The one exception today is the theme picker: `ctrl+alt+t` opens it on both platforms, but Windows also binds `ctrl+alt+shift+t` to the same action, since `ctrl+alt+t` can be intercepted by other software on some Windows setups. The [keybindings reference](../reference/keybindings.md) marks every row's platform explicitly, so you can tell at a glance whether a chord is universal or Windows-only.

## Discovering action names

The [keybindings reference](../reference/keybindings.md) table lists every action that has a default binding, alongside the chord that triggers it. One action has no default chord and so does not appear there: `goto_tab_1` through `goto_tab_<N>` jump directly to tab `N` (1-based; a number past the last open tab jumps to the last one). Bind whichever numbers you use, for example:

```toml
[keys]
"ctrl+alt+1" = "goto_tab_1"
"ctrl+alt+2" = "goto_tab_2"
```
