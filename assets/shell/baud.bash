# Baud shell integration for bash. Injected via --rcfile when
# shell_integration = "auto"; safe to source manually as well.
[[ -n "${BAUD_SHELL_INTEGRATION_DONE-}" ]] && return
[[ "$TERM" == "xterm-256color" ]] || return
BAUD_SHELL_INTEGRATION_DONE=1

__baud_precmd() {
  local status=$?
  printf '\033]133;D;%d\007\033]133;A\007' "$status"
}

__baud_preexec() {
  [[ -n "${COMP_LINE-}" ]] && return
  [[ -n "${__baud_preexec_done-}" ]] && return
  __baud_preexec_done=1
  printf '\033]133;C\007'
}

__baud_user_prompt_command=${PROMPT_COMMAND-}

__baud_prompt_wrapper() {
  __baud_in_prompt=1
  __baud_preexec_done=
  __baud_precmd
  if [[ -n "${__baud_user_prompt_command-}" ]]; then
    eval "$__baud_user_prompt_command"
  fi
  __baud_in_prompt=
}

PROMPT_COMMAND=__baud_prompt_wrapper

PS1="${PS1-}"$'\033]133;B\007'

__baud_prior_debug=
__baud_debug_spec=$(trap -p DEBUG 2>/dev/null || true)
if [[ "$__baud_debug_spec" == trap\ --\ \'*\'\ DEBUG ]]; then
  __baud_prior_debug=${__baud_debug_spec#trap -- \'}
  __baud_prior_debug=${__baud_prior_debug%\' DEBUG}
elif [[ "$__baud_debug_spec" == trap\ --\ \"*\"\ DEBUG ]]; then
  __baud_prior_debug=${__baud_debug_spec#trap -- \"}
  __baud_prior_debug=${__baud_prior_debug%\" DEBUG}
fi
unset __baud_debug_spec

__baud_debug() {
  if [[ -n "${__baud_in_prompt-}" || -n "${COMP_LINE-}" || "$BASH_COMMAND" == "__baud_prompt_wrapper" ]]; then
    if [[ -n "${__baud_prior_debug-}" ]]; then
      eval "$__baud_prior_debug"
    fi
    return
  fi
  __baud_preexec
  if [[ -n "${__baud_prior_debug-}" ]]; then
    eval "$__baud_prior_debug"
  fi
}
# El trap va al final: si se instala antes, la asignacion de PS1 emite C.
trap '__baud_debug' DEBUG
