# Baud shell integration for bash. Injected via --rcfile when
# shell_integration = "auto"; safe to source manually as well.
[[ -n "${BAUD_SHELL_INTEGRATION_DONE-}" ]] && return
[[ "$TERM" == "xterm-256color" ]] || return
BAUD_SHELL_INTEGRATION_DONE=1

__baud_osc_b=$'\033]133;B\007'

__baud_precmd() {
  printf '\033]133;D;%d\007\033]133;A\007' "$1"
}

__baud_preexec() {
  [[ -n "${COMP_LINE-}" ]] && return
  [[ -n "${__baud_preexec_done-}" ]] && return
  __baud_preexec_done=1
  printf '\033]133;C\007'
}

__baud_ensure_ps1_b() {
  local b="$__baud_osc_b"
  PS1="${PS1//"$b"/}"
  PS1="${PS1}${b}"
}

__baud_user_pc_array=()
__baud_save_prompt_command() {
  __baud_user_pc_array=()
  local spec
  spec=$(declare -p PROMPT_COMMAND 2>/dev/null || true)
  if [[ "$spec" == declare\ -a* ]]; then
    local item
    for item in "${PROMPT_COMMAND[@]}"; do
      [[ "$item" == "__baud_prompt_wrapper" ]] && continue
      __baud_user_pc_array+=("$item")
    done
  elif [[ -n "${PROMPT_COMMAND-}" && "${PROMPT_COMMAND}" != "__baud_prompt_wrapper" ]]; then
    __baud_user_pc_array=("${PROMPT_COMMAND}")
  fi
  # unset first: assigning a string only replaces [0] of an existing array.
  unset PROMPT_COMMAND
  PROMPT_COMMAND=__baud_prompt_wrapper
}

__baud_run_user_prompt_command() {
  local status=$1
  local cmd
  for cmd in "${__baud_user_pc_array[@]}"; do
    [[ -z "$cmd" || "$cmd" == "__baud_prompt_wrapper" ]] && continue
    (exit "$status")
    eval "$cmd"
  done
}

__baud_prompt_wrapper() {
  # Capture before any assignment: those would force $? to 0.
  __baud_last_status=$?
  __baud_in_prompt=1
  __baud_preexec_done=
  __baud_precmd "$__baud_last_status"
  __baud_run_user_prompt_command "$__baud_last_status"
  __baud_ensure_ps1_b
  __baud_in_prompt=
}

__baud_save_prompt_command

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
trap '__baud_debug' DEBUG
