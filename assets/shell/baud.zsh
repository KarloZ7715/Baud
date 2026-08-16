# Baud shell integration for zsh. Injected via ZDOTDIR when
# shell_integration = "auto"; safe to source manually as well.
[[ -n "${BAUD_SHELL_INTEGRATION_DONE-}" ]] && return
[[ "$TERM" == "xterm-256color" ]] || return
BAUD_SHELL_INTEGRATION_DONE=1

__baud_osc_b=$'\033]133;B\a'

__baud_precmd() {
  print -Pn "\e]133;D;$?\a\e]133;A\a"
  # Re-apply B every cycle: a user precmd may have rebuilt PS1.
  PS1=${PS1//$'%{\033]133;B\a%}'/}
  PS1=${PS1//$'\033]133;B\a'/}
  PS1="${PS1}%{${__baud_osc_b}%}"
}
__baud_preexec() { print -Pn "\e]133;C\a" }
precmd_functions+=(__baud_precmd)
preexec_functions+=(__baud_preexec)
