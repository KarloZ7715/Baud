# Remote control

Remote control lets another process on the same machine read the screen as text and inject keystrokes. It is **off by default**. You opt in with `remote_control = true` in your config — see the [configuration reference](../reference/config.md#config-section-root).

The MCP adapter (`baud mcp`) is a thin client of the same local socket. It does not add network access or a second permission model.

> [!WARNING]
> Enabling remote control allows any process running as your user to read the terminal screen and inject keys for as long as that Baud instance is open.

## Security model

- **Opt-in.** With the default `remote_control = false`, Baud does not create a listener.
- **Local only.** The control channel is a Unix domain socket on Linux, or a named pipe on Windows. There is no TCP or other network listener.
- **Token.** Each instance writes a random 32-byte token to a `0600` file next to the socket. The first message on a connection must be `hello` with that token; a mismatch closes the connection.
- **Private directory.** On Linux the socket lives in `$XDG_RUNTIME_DIR/baud/<pid>.sock` (directory mode `0700`). On Windows the pipe name is `\\.\pipe\baud-<pid>-<short-token>` and the token file lives under `%LOCALAPPDATA%\baud\runtime\`.

Owning the token file is the authorization. The MCP process is one more local client: it reads the same file and speaks the same protocol.

## Protocol v1

One JSON object per line. A request is `{"id": 1, "method": "screen_text", "params": {}}`. A success is `{"id": 1, "ok": {...}}`. A failure is `{"id": 1, "err": {"code": "...", "msg": "..."}}`. Malformed lines return `err` and leave the connection open.

| Method | Params | Response |
| --- | --- | --- |
| `hello` | `token` | `version`, `pid`. Required first; authenticates the connection. |
| `list_sessions` | — | Tabs, panes, ids, titles, focus, and each pane's `cols`/`rows`. |
| `screen_text` | `session?`, `scrollback_lines?`, `start_row?`, `end_row?` | `lines` plus `cols`, `rows`, `total_rows` (scrollback + visible), and `start_row` (absolute index, 0 = oldest scrollback line). Without a range, trailing blank lines are trimmed. `start_row`/`end_row` are inclusive absolute indices and override `scrollback_lines`. |
| `screen_detail` | `session?`, `rect?`, `detail?` | `detail` is `compact` (default: style runs per row) or `full` (one object per cell). Both include `cursor` and `modes` (`alt_screen`, `mouse`, `extended_keyboard`, `bracketed_paste`). |
| `send_text` | `session?`, `text`, `bracketed?` | Writes `text` to the PTY. Reply is `written`, `session`, `focused`, and `bracketed` (true only if requested and the app enabled that mode). |
| `send_key` | `session?`, `chord` | Encodes `chord` with the same grammar as config keybindings (`ctrl+c`, `enter`) and writes it. Reply includes `written`, `session`, and `focused`. |
| `wait_for` | `session?`, `pattern`, `timeout_ms?` | Waits until the **visible** screen contains `pattern`. Match: `{"matched": true}`. Timeout: `{"matched": false, "timed_out": true}` (success, not `err`). Scrollback is not searched. |
| `wait_idle` | `session?`, `idle_ms?`, `timeout_ms?` | Waits until the visible screen is unchanged for `idle_ms` (default 500). Success: `{"idle": true}`. Timeout: `{"idle": false, "timed_out": true}`. Default `timeout_ms` is 5000. |
| `get_config` | — | Effective configuration as JSON. Baud has no secrets in config. |
| `tail_log` | `lines` | Last N lines of today's log file. |

`session` is a pane id from `list_sessions`. Omit it to target the focused pane. `rect` is `{x, y, cols, rows}` in cell coordinates; omit it for the full visible grid.

## `baud mcp`

`baud mcp` speaks [Model Context Protocol](https://modelcontextprotocol.io/) over stdin/stdout and translates each tool call into the protocol above. With no arguments it attaches to the most recently started **live** instance that has remote control enabled (dead sockets are skipped and removed). `--socket <path>` selects a specific socket. `--list-tools` prints the tool catalog as JSON and exits 0 without connecting to any instance.

If no instance is listening, it prints `no running baud instance with remote_control enabled` to stderr and exits with status 1.

A client that launches MCP servers (for example Claude Code or Codex) can register:

```json
{
  "command": "baud",
  "args": ["mcp"]
}
```

The tools are `baud_list_sessions`, `baud_screen`, `baud_screen_detail`, `baud_send_text`, `baud_send_key`, `baud_wait_for`, `baud_wait_idle`, `baud_get_config`, and `baud_tail_log`. Each maps one-to-one onto the methods in the table.

## What this does not do

Remote control does not capture pixels, accept connections over the network, or spawn new sessions. You start Baud as usual; the socket only appears when `remote_control` is true.
