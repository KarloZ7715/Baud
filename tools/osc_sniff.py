#!/usr/bin/env python3
"""Registra las secuencias OSC/CSI/DCS que una aplicacion emite al arrancar.

Abre un pty crudo sin emulador detras, asi que ninguna consulta recibe
respuesta: sirve para ver que le pide una TUI al terminal y cuantas veces lo
reintenta. Uso:

    python3 tools/osc_sniff.py opencode
"""
import os
import pty
import re
import select
import sys
import time

PATRON = re.compile(
    rb"\x1b\][0-9]+;[^\x07\x1b]{0,60}\x07"
    rb"|\x1b\][0-9]+;[^\x07\x1b]{0,60}\x1b\\"
    rb"|\x1bP[^\x1b]{0,60}\x1b\\"
    rb"|\x1b\[[?>=0-9;]*[a-zA-Z]"
)


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2
    cmd = sys.argv[1:]
    pid, fd = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        os.environ["COLORTERM"] = "truecolor"
        os.environ.pop("COLORFGBG", None)
        os.execvp(cmd[0], cmd)

    buf = b""
    limite = time.time() + 8
    while time.time() < limite:
        listo, _, _ = select.select([fd], [], [], 0.3)
        if not listo:
            continue
        try:
            trozo = os.read(fd, 65536)
        except OSError:
            break
        if not trozo:
            break
        buf += trozo

    os.kill(pid, 9)
    os.waitpid(pid, 0)

    vistas: dict[bytes, int] = {}
    for seq in PATRON.findall(buf):
        vistas[seq] = vistas.get(seq, 0) + 1
    for seq, veces in vistas.items():
        sufijo = f"  (x{veces})" if veces > 1 else ""
        print(f"{seq!r}{sufijo}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
