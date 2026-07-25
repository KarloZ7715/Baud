# Tabs and splits

## Tabs

Each tab is an independent session. Its label comes from the shell's OSC window-title updates, shortened automatically to a readable name (for example, a long path collapses to its last component). Click a tab to switch to it, or hover to reveal a close (×) button. When there are more tabs than fit the window, the bar scrolls and shows `‹`/`›` indicators, always keeping the focused tab visible.

| Action | Default chord |
| --- | --- |
| New tab | `ctrl+shift+t` |
| Close tab | `ctrl+shift+w` |
| Next / previous tab | `ctrl+pagedown` / `ctrl+pageup` |
| Jump to tab `N` | none by default — bind `goto_tab_1`..`goto_tab_<N>`, see [Keybindings](../config/keybindings.md#discovering-action-names) |

## Splits

Splitting divides the focused pane in two, each running its own shell. There is no default chord for opening a *new* pane beyond splitting — every pane starts as a split of an existing one.

| Action | Default chord |
| --- | --- |
| Split pane | `ctrl+shift+d` |
| Toggle split orientation | `ctrl+shift+\|` |
| Swap the two panes of a split | `ctrl+shift+s` |
| Focus next / previous pane | `ctrl+shift+]` / `ctrl+shift+[` |
| Focus pane up/down/left/right | `alt+shift+up`/`down`/`left`/`right` |
| Close focused pane | `ctrl+shift+q` |

Focusing by direction (`focus_pane_*`) picks the geometrically closest neighbor in that direction; if more than one pane is adjacent, the one most recently focused wins the tie.

### Orientation

By default (`panes.smart_split = false`), a new split's orientation follows a dwindle-style rule (as in Hyprland): it alternates based on the focused pane's aspect ratio, tuned by `panes.split_width_multiplier`. With `panes.smart_split = true`, the orientation instead follows where your mouse cursor sits inside the pane when you trigger the split — imagine the pane divided into four triangles from its center; the triangle your cursor is in decides both the split axis and which side keeps the existing content.

`panes.preserve_split` (implied `true` whenever `smart_split` is on) stops Baud from recalculating a pane's split orientation when the window is resized, keeping whatever orientation you split it with.

`panes.max` caps how many panes a single tab can hold (default `12`; `0` removes the limit). Splitting past the limit, or into a pane too small to hold two panes, shows a status message instead of silently failing.

See the [configuration reference](../reference/config.md#config-section-panes) for every `panes.*` key, and [Keybindings](../config/keybindings.md) for rebinding any of the chords above.
