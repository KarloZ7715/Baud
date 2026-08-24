# fish completion for baud
# -e --working-directory --title --app-id --hold --config --window-size --maximized --fullscreen -o

function __baud_no_e
    not contains -- -e (commandline -opc)
end

complete -c baud -n __baud_no_e -a update -d "Update Baud to the latest release"
complete -c baud -n __baud_no_e -a version -d "Print the installed Baud version"
complete -c baud -n __baud_no_e -a mcp -d "Speak MCP over stdio to a running Baud instance"
complete -c baud -n __baud_no_e -a help -d "Show this help message"

complete -c baud -n __baud_no_e -s e -d "Execute command and its arguments in the PTY" -xa "(__fish_complete_command)"
complete -c baud -n __baud_no_e -l working-directory -d "Set the initial working directory" -xa "(__fish_complete_directories)"
complete -c baud -n __baud_no_e -l title -d "Set the initial window title" -r
complete -c baud -n __baud_no_e -l app-id -d "Set the Wayland app_id / X11 WM_CLASS instance" -r
complete -c baud -n __baud_no_e -l hold -d "Keep the window open after the command exits"
complete -c baud -n __baud_no_e -l config -d "Load config from this file" -r
complete -c baud -n __baud_no_e -l window-size -d "Set the initial window size in terminal cells" -r
complete -c baud -n __baud_no_e -l maximized -d "Start the window maximized"
complete -c baud -n __baud_no_e -l fullscreen -d "Start the window in borderless fullscreen"
complete -c baud -n __baud_no_e -s o -d "Override a config key (repeatable)" -r
complete -c baud -n __baud_no_e -s v -l version -d "Print the installed Baud version"
complete -c baud -n __baud_no_e -s h -l help -d "Show this help message"

complete -c baud -n "__baud_no_e; and __fish_seen_subcommand_from mcp" -l socket -d "Control socket" -r
complete -c baud -n "__baud_no_e; and __fish_seen_subcommand_from mcp" -l list-tools -d "Print the MCP tool catalog as JSON and exit"
