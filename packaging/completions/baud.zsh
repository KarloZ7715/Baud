#compdef baud

_arguments -s -S \
  '(- *)-e[Execute command and its arguments in the PTY]:*:command:_command_names' \
  '--working-directory[Set the initial working directory]:dir:_directories' \
  '--title[Set the initial window title]:text:' \
  '--app-id[Set the Wayland app_id / X11 WM_CLASS instance]:id:' \
  '--hold[Keep the window open after the command exits]' \
  '--config[Load config from this file]:path:_files' \
  '--window-size[Set the initial window size in terminal cells]:COLSxROWS:' \
  '--maximized[Start the window maximized]' \
  '--fullscreen[Start the window in borderless fullscreen]' \
  '*-o[Override a config key (repeatable)]:key=value:' \
  '(-v --version)'{-v,--version}'[Print the installed Baud version]' \
  '(-h --help)'{-h,--help}'[Show this help message]' \
  '1:command:(update version mcp help)' \
  '*::mcp-args:->mcp'

case $state in
  mcp)
    _arguments \
      '--socket[Control socket]:path:_files' \
      '--list-tools[Print the MCP tool catalog as JSON and exit]'
    ;;
esac
