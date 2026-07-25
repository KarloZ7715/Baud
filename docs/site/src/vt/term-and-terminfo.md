# TERM and terminfo

Baud sets `TERM=xterm-256color` and `COLORTERM=truecolor` for the child process, on both Linux and Windows. This is forced unconditionally — even a `TERM` or `COLORTERM` value set in `process.env` (see the [configuration reference](../reference/config.md#config-section-process)) is overridden, so there is no way to make Baud claim a different terminal type.

## What this means for applications

Programs that check `$TERM` will negotiate capabilities against the well-known `xterm-256color` terminfo entry, which every Unix system already ships. `COLORTERM=truecolor` is the de facto signal (used by Vim, tmux, and others) that 24-bit color is safe to use, advertised separately from the terminfo capability set. Applications that query capabilities directly with `XTGETTCAP` (`\eP+q...`) get real answers from Baud itself for a small set of keys — `RGB`, `Tc`, and `colors`/`Co` — rather than whatever `xterm-256color` implies; see the [Terminal API](index.md) page for the exact set.

## Why there's no `baud` terminfo entry

Because Baud presents itself as `xterm-256color`, there is nothing for an application to look up under a `baud` entry, and none needs installing. This matters most over SSH: connecting from a Baud session to a remote host works immediately, without copying a custom terminfo database to that host first — a common source of `unknown terminal type` errors for terminals that ship their own distinct `TERM` value.

## The tradeoff

Baud's own extensions (OSC 133 semantic prompts, the kitty keyboard protocol subset, bracketed paste, synchronized output) are real but invisible to `TERM`-based capability detection — an application can't discover them by checking `$TERM`, only by probing the escape sequences directly or checking `XTGETTCAP`. This is the same tradeoff every xterm-compatible terminal that skips its own terminfo entry makes.
