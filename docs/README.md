```yaml
titulo: "Documentacion , Baud"
tipo: guia
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-13"
fecha_modificacion: "2026-06-15"
version: "0.3.0"
estado: publicado
tags: [indice, documentacion, terminal, rust, baud]
```

# Documentacion , Baud

---

## Indice

### Decisiones

- [ADR-0001: Estructura de investigacion](decisions/ADR-0001-estructura-investigacion.md)
- [ADR-0002: Organizacion de documentacion](decisions/ADR-0002-organizacion-documentacion.md)
- [ADR-0003: Estructura del codigo en 3 capas](decisions/ADR-0003-estructura-codigo.md)
- [ADR-0004: Seleccion final de crates](decisions/ADR-0004-seleccion-crates.md)
- [ADR-0005: Patron de event loop e integracion I/O](decisions/ADR-0005-event-loop-io.md)
- [ADR-0006: Estrategia de testing](decisions/ADR-0006-testing-strategy.md)
- [ADR-0007: Error handling y robustez](decisions/ADR-0007-error-handling.md)
- [ADR-0008: Roadmap de implementacion y MVP](decisions/ADR-0008-roadmap-mvp.md)

### Especificaciones

- [Requisitos (RF/RNF)](specs/requisitos.md)
- [Estrategia de Testing](specs/testing-strategy.md)
- [Error Handling](specs/error-handling.md)
- [Roadmap Operativo](specs/roadmap.md)

---

## Stack Tecnologico


| Capa             | Tecnologia  | Version | Documento |
| ---------------- | ----------- | ------- | --------- |
| Parser ANSI      | vte         | 0.15    | ADR-0004  |
| PTY              | nix         | 0.31    | ADR-0004  |
| Ventana          | winit       | 0.30    | ADR-0004  |
| Render GPU       | wgpu        | 29      | ADR-0004  |
| Texto            | glyphon     | 0.11    | ADR-0004  |
| Logging          | tracing     | 0.1     | ADR-0007  |
| Errores (borde)  | anyhow      | 1       | ADR-0007  |
| Errores (domain) | thiserror   | 2       | ADR-0007  |
| Lock             | parking_lot | 0.12    | ADR-0005  |


**MSRV efectiva:** 1.87.0 (impuesta por wgpu).

**Patron de event loop:** 2 hilos (GUI + PTY) sin async runtime. Detalles en ADR-0005.

**Arquitectura:** 3 capas (Presentation, Domain, Infrastructure). Detalles en ADR-0003.

**Testing:** 4 niveles (unit + integration + proptest + vttest) + benchmarks con criterion + CI en GitHub Actions. Detalles en ADR-0006.

**Roadmap:** 6 fases (Fase 0 a Fase 5), MVP en Fase 3 (4 sprints). Detalles en ADR-0008 y `docs/specs/roadmap.md`.

---

*Ultima actualizacion: 2026-06-15*

## Cambios


| Version | Fecha      | Cambios                                                                                                                                                                                                                         |
| ------- | ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0.1.0   | 2026-06-13 | Creacion inicial del indice                                                                                                                                                                                                     |
| 0.2.0   | 2026-06-14 | Iter 6: agregados links a 06-arquitectura.md, 6 nuevos ADRs (0003-0008), 4 specs (requisitos, testing-strategy, error-handling, roadmap), 6 archivos de investigacion de subagentes (A-F). Tabla de stack tecnologico agregada. |
| 0.3.0   | 2026-06-15 | Renombrado del proyecto: Terminal Emulator en Rust pasa a llamarse **Baud** (unidad de velocidad de transmision, en honor a Emile Baudot 1845-1903).                                                                            |


