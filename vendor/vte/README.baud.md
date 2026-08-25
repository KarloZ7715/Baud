# vte 0.15.0 (parche local)

Copia de `vte` 0.15.0 con entrega de cadenas APC al trait `Perform`
(`apc_start` / `apc_put` / `apc_end`). El crate de crates.io descarta esas
cadenas en el estado `SosPmApcString` sin callbacks.

El parche vive en `src/lib.rs`. SOS (`ESC X`) y PM (`ESC ^`) siguen sin
entregarse.
