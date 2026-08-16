# Shell integration

Baud recognizes the [OSC 133](https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.html) semantic prompt sequences. With a full A/B/C/D stream it can tell prompt, input, and output apart. Today that is used to jump between prompts; later features (blocks, copy last output) will sit on the same marks.

| Action | Default chord |
| --- | --- |
| Jump to previous prompt | `ctrl+alt+up` |
| Jump to next prompt | `ctrl+alt+down` |

Marks are ignored while an alternate-screen program is active, so they never point into `vim` or `less` output.

## Automatic injection

By default (`shell_integration = "auto"`), Baud injects integration for **bash** and **zsh** when it launches the shell:

- **zsh** — sets `ZDOTDIR` to a small runtime directory whose `.zshrc` sources yours and then emits the marks. If you already had `ZDOTDIR` set, it is preserved as `BAUD_ORIG_ZDOTDIR`.
- **bash** — starts the shell with `--rcfile` pointing at a wrapper that sources `~/.bashrc` and then emits the marks. This is skipped when `[process].args` is non-empty or `process.login = true`, because `--rcfile` would fight those.
- **PowerShell** — only sets `BAUD_SHELL_INTEGRATION=1` and `BAUD_SHELL_INTEGRATION_SCRIPT` to the embedded snippet. Add this line to your `$PROFILE`:

```powershell
if ($env:BAUD_SHELL_INTEGRATION -eq '1' -and $env:BAUD_SHELL_INTEGRATION_SCRIPT) {
    . $env:BAUD_SHELL_INTEGRATION_SCRIPT
}
```

- **Anything else** (fish, a custom `process.program`, …) — the child environment is left alone.

Turn it off to get a child environment that is byte-for-byte what you configured:

```toml
shell_integration = "off"
```

If the shell is not recognized, or the runtime scripts cannot be written, Baud starts the shell as-is.

The scripts only emit marks while `TERM` is still `xterm-256color` (the value Baud sets). If you change `TERM` inside the session, they stay quiet.

## Manual setup

The same scripts Baud injects are written to the state directory on first launch (`~/.local/state/baud/shell-integration/` on Linux, `%LOCALAPPDATA%\baud\shell-integration\` on Windows). They are safe to source twice. They emit:

| Mark | When |
| --- | --- |
| `OSC 133;D;<exit>` | Previous command finished |
| `OSC 133;A` | Prompt is about to be drawn |
| `OSC 133;B` | Prompt finished; input starts |
| `OSC 133;C` | Command is about to run |

With `shell_integration = "off"`, source `bash/baud.bash` from `~/.bashrc` or `zsh/baud.zsh` from `~/.zshrc`. A compact zsh form if you prefer not to source the file:

```sh
__baud_precmd() { print -Pn "\e]133;D;$?\a\e]133;A\a" }
__baud_preexec() { print -Pn "\e]133;C\a" }
precmd_functions+=(__baud_precmd)
preexec_functions+=(__baud_preexec)
PS1="${PS1}%{$(print -Pn "\e]133;B\a")%}"
```

Capturing `$?` as the first thing in the precmd hook is what makes the exit-code mark reflect the command that just finished.

### Other shells

Any shell that can run a hook before redrawing the prompt can emit `\033]133;D;<exit-code>\007` then `\033]133;A\007`, wrap the prompt with `\033]133;B\007`, and emit `\033]133;C\007` just before running a command.
