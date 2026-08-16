# Baud shell integration for zsh. Injected via ZDOTDIR when
# shell_integration = "auto"; safe to source manually as well.
[[ -n "${BAUD_SHELL_INTEGRATION_DONE-}" ]] && return
[[ "$TERM" == "xterm-256color" ]] || return
BAUD_SHELL_INTEGRATION_DONE=1

__baud_precmd() { print -Pn "\e]133;D;$?\a\e]133;A\a" }
__baud_preexec() { print -Pn "\e]133;C\a" }
precmd_functions+=(__baud_precmd)
preexec_functions+=(__baud_preexec)
# B marks the end of the prompt so input sits between B and C.
PS1="${PS1}%{$(print -Pn "\e]133;B\a")%}"
