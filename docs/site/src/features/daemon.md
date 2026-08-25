# Session daemon

`baud` with no flags is a short-lived client. It asks a session daemon for a tab (or the first window, if none is open) and then exits. The daemon keeps fonts and the GPU device alive so the next launch does not pay that cost again.

Closing every tab does not stop the daemon. The next `baud` reuses the process that is already running.

## How the first window of the day behaves

If the session service is already running at login, the first window opens without scanning fonts again.

If it is not, the first `baud` starts `baud --server` for you, waits until the spawn socket accepts connections, sends the request, and exits. That first launch still pays the font scan once. Later launches in the same session do not.

## Autostart

Linux packages ship a user systemd unit (`baud.service`) and an XDG autostart entry. Enable the unit in your graphical session with:

```sh
systemctl --user enable --now baud.service
```

The MSI installer places a Startup folder shortcut that runs `baud --server`. A portable zip or `cargo run` has no autostart; the first `baud` starts the daemon.

## Flags

| Flag | Effect |
| --- | --- |
| `--server` | Run the session daemon in the foreground. A second `--server` while one is already holding the spawn socket exits 0. |
| `--new-instance` | Open a GUI that does not talk to the daemon. Closing its last tab ends that process. |

Launch flags such as `-e`, `--working-directory`, `--title`, `--hold`, and `--app-id` travel with the client request. `--config` and `-o` apply to a daemon this client starts; they do not change a daemon that is already running.

## Spawn socket vs remote control

The spawn channel is always on while the daemon lives. It only accepts `hello` and `new_tab`. It does not read the screen or inject keys.

[Remote control](remote-control.md) stays opt-in (`remote_control = true`) and uses a different socket. Enabling it is not required for the daemon.

## What this does not do

The daemon does not multiplex several OS windows, detach a session from the GUI, or expose spawn through `baud mcp`. `--new-instance` is the escape hatch when you want a process that dies with its window.
