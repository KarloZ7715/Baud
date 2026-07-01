#!/usr/bin/env bash
# Grids de referencia para comparación manual baud vs foot.
set -euo pipefail

echo "=== 1. Tabla 2x2 (light) ==="
printf '┌───┬───┐\n│ A │ B │\n├───┼───┤\n│ C │ D │\n└───┴───┘\n'

echo "=== 2. Round border (Hermes style) ==="
printf '╭───────────╮\n│           │\n╰───────────╯\n'

echo "=== 3. Double line ==="
printf '╔═══╗\n║   ║\n╚═══╝\n'

echo "=== 4. Vertical junction ==="
printf '┬\n│\n┼\n│\n┴\n'

echo "=== 5. Block elements ==="
printf '▛▟ ░▒▓█\n▂▃▄▅▆▇█\n'

echo "=== 6. Half blocks stacked ==="
printf '▀▀▀▀▀▀▀▀\n▄▄▄▄▄▄▄▄\n'

echo "=== 7. Heavy + light mix ==="
printf '┏━┓\n┃ ┃\n┗━┛\n'

echo "=== 8. Dashed (Hermes custom) ==="
printf '╌╌╌╌╌\n╎   ╎\n╌╌╌╌╌\n'
