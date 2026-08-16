# Helpers for the injected ZDOTDIR wrappers. Sourced from each startup file.
__baud_user_zdot="${BAUD_ORIG_ZDOTDIR:-$HOME}"
: "${__baud_inject_zdot:=$ZDOTDIR}"

__baud_source_user() {
  local f="${__baud_user_zdot}/$1"
  [[ -r "$f" ]] || return 0
  if [[ -n "${BAUD_ORIG_ZDOTDIR+x}" ]]; then
    ZDOTDIR="$BAUD_ORIG_ZDOTDIR"
  else
    unset ZDOTDIR
  fi
  source "$f"
}

__baud_stay_injected() {
  ZDOTDIR="$__baud_inject_zdot"
}

__baud_restore_user_zdot() {
  if [[ -n "${BAUD_ORIG_ZDOTDIR+x}" ]]; then
    export ZDOTDIR="$BAUD_ORIG_ZDOTDIR"
  else
    unset ZDOTDIR
  fi
}
