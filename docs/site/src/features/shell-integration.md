# Shell integration

Baud recognizes the [OSC 133](https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.html) "semantic prompt" escape sequence. When your shell emits an `A` mark at the start of each prompt, Baud records that row so you can jump between prompts:

| Action | Default chord |
| --- | --- |
| Jump to previous prompt | `ctrl+alt+up` |
| Jump to next prompt | `ctrl+alt+down` |

Baud does not ship a bundled shell hook — add one of the snippets below to your shell's startup file. Only the `A` mark (new prompt) affects anything today; the `D` mark (previous command's exit code) is parsed and stored per-prompt but not yet surfaced anywhere in the UI. Marks are ignored while an alternate-screen program (an editor, a pager) is active, so they never point into `vim` or `less` output.

## Bash

Add to `~/.bashrc`:

```sh
__baud_precmd() { printf '\033]133;D;%d\007\033]133;A\007' "$?"; }
PROMPT_COMMAND="__baud_precmd${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
```

Capturing `$?` as the very first thing in the function is what makes the exit-code mark reflect the command that just finished, rather than whatever ran last inside `PROMPT_COMMAND` itself.

## Zsh

Add to `~/.zshrc`:

```sh
__baud_precmd() { print -Pn "\e]133;D;$?\a\e]133;A\a" }
precmd_functions+=(__baud_precmd)
```

## Other shells

Any shell that can run a hook right before it redraws the prompt can emit the same two sequences: `\033]133;D;<exit-code>\007` followed by `\033]133;A\007`.
