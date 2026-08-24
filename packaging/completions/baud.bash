# bash completion for baud

_baud() {
    local cur prev
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    local i
    for ((i = 1; i < COMP_CWORD; i++)); do
        if [[ "${COMP_WORDS[i]}" == "-e" ]]; then
            COMPREPLY=($(compgen -c -- "$cur"))
            return 0
        fi
    done

    case "$prev" in
        --working-directory|--config)
            COMPREPLY=($(compgen -d -- "$cur"))
            return 0
            ;;
        --title|--app-id|--window-size|-o|--socket)
            return 0
            ;;
        mcp)
            COMPREPLY=($(compgen -W "--socket --list-tools" -- "$cur"))
            return 0
            ;;
    esac

    if [[ "$cur" == -* ]]; then
        COMPREPLY=($(compgen -W "-e --working-directory --title --app-id --hold --config --window-size --maximized --fullscreen --server --new-instance -o -v --version -h --help --socket --list-tools" -- "$cur"))
        return 0
    fi

    COMPREPLY=($(compgen -W "update version mcp help" -- "$cur"))
}

complete -F _baud baud
