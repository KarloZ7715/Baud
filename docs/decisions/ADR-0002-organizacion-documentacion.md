```yaml
titulo: "Organizacion de Documentacion del Proyecto"
tipo: decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "2026-06-13"
fecha_modificacion: "2026-06-14"
version: "0.2.0"
estado: borrador
tags: [decision, organizacion, estructura, documentacion, proyecto]
```

# ADR: Organizacion de Documentacion del Proyecto

## Contexto

Un proyecto de ingenieria de software de gran escala genera
múltiples tipos de documentación: investigacion, diseno,
especificaciones técnicas, decisiones, testing, y documentacion
de usuario. Sin una estructura clara, los documentos se
pierden, se vuelven inconsistentes, o nadie los encuentra.

Las fuentes consultadas incluyen:

- ISO/IEC/IEEE 26514:2022 (documentacion de software)
- Best practices de AltexSoft, MIT Broad Institute
- Estructura de proyectos open source (Alacritty, Rust, Python)
- Framework Diataxis (documentacion técnica)

---

## Principios de organizacion

### 1. Separacion de responsabilidades

Cada carpeta tiene un proposito único. No mezclar
investigacion con diseno, ni decisiones con especificaciones.

### 2. Consistencia

Nombres de archivos, formato, y estructura deben ser iguales
en todo el proyecto. Un archivo nuevo debe seguir el patron
de los existentes.

### 3. Escalabilidad

La estructura debe crecer con el proyecto. Si hoy hay 3
documentos y manana 30, la estructura debe soportarlo sin
reorganizarse.

### 4. Predictabilidad

Cualquier persona (o IA) debe poder encontrar un documento
sabiendo que busca, sin tener que explorar toda la carpeta.

### 5. Separar código de documentacion

La documentacion vive junto al código pero en carpetas
separadas. El código fuente esta en `src/`, la documentacion
en `docs/`.

---

## Estructura recomendada

```text
baud/
│
├── Cargo.toml                    # Manifesto de Rust
├── Cargo.lock                    # Dependencias lock
├── .gitignore
├── .markdownlint.yaml            # Config de linting
├── README.md                     # Vision general del proyecto
├── LICENSE
├── CHANGELOG.md                  # Historial de cambios
├── CONTRIBUTING.md               # Como contribuir
│
├── docs/                         # Toda la documentacion
│   ├── README.md                 # Indice de documentacion
│   │
│   ├── standards/                # Estandares y procesos
│   │   ├── documentacion.md      # Como escribir documentos
│   │   └── metodologia.md        # Como investigar
│   │
│   ├── decisions/                # Decisiones arquitectonicas (ADRs)
│   │   ├── ADR-0001-*.md
│   │   └── ADR-0002-*.md
│   │
│   ├── research/                 # Investigacion tecnica
│   │   ├── 00-fundamentos.md
│   │   ├── 01-pty-shell.md
│   │   ├── 02-rendering.md
│   │   ├── 03-input.md
│   │   ├── 04-ansi-parser.md
│   │   ├── 05-terminal-grid.md
│   │   └── 06-arquitectura.md
│   │
│   ├── design/                   # Diseno tecnico
│   │   ├── architecture.md       # Arquitectura general
│   │   ├── modules/              # Diseno de modulos
│   │   └── data-models/          # Modelos de datos
│   │
│   ├── specs/                    # Especificaciones tecnicas
│   │   ├── pty-manager.md
│   │   ├── renderer.md
│   │   └── input-handler.md
│   │
│   ├── testing/                  # Estrategia de testing
│   │   ├── test-plan.md
│   │   └── test-cases.md
│   │
│   └── references/               # Fuentes externas
│       ├── proyectos.md
│       └── recursos.md
│
├── src/                          # Codigo fuente
│   ├── main.rs
│   ├── lib.rs
│   ├── pty/                      # Modulo PTY
│   ├── renderer/                 # Modulo rendering
│   ├── input/                    # Modulo input
│   ├── ansi/                     # Parser ANSI
│   ├── grid/                     # Terminal grid
│   └── config/                   # Configuracion
│
├── tests/                        # Tests
│   ├── integration/
│   └── unit/
│
└── benches/                      # Benchmarks
    └── render.rs
```

---

## Tipos de documento y su ubicacion


| Tipo               | Carpeta       | Proposito                      | Ejemplo            |
| ------------------ | ------------- | ------------------------------ | ------------------ |
| **Estandar**       | `standards/`  | Como hacer las cosas           | `documentacion.md` |
| **Decision**       | `decisions/`  | Que se decidio y por que       | `ADR-0001-*.md`    |
| **Investigacion**  | `research/`   | Que se descubrio               | `01-pty-shell.md`  |
| **Diseno**         | `design/`     | Como se construye              | `architecture.md`  |
| **Especificacion** | `specs/`      | Que debe hacer cada componente | `pty-manager.md`   |
| **Testing**        | `testing/`    | Como se verifica               | `test-plan.md`     |
| **Referencia**     | `references/` | Fuentes externas               | `recursos.md`      |


---

## Convenciones de nombres

### Archivos


| Tipo          | Convencion           | Ejemplo                  |
| ------------- | -------------------- | ------------------------ |
| Investigacion | `NN-tema.md`         | `01-pty-shell.md`        |
| ADR           | `ADR-NNNN-titulo.md` | `ADR-0001-estructura.md` |
| Spec          | `componente.md`      | `pty-manager.md`         |
| Diseno        | `tema.md`            | `architecture.md`        |


### Fechas

Formato: `YYYY-MM-DD` en front matter, no en nombres de archivo.

### Versiones

Semver en front matter: `0.1.0` (borrador), `1.0.0` (publicado).

---

## Front matter estándar

TODO documento DEBE tener:

```yaml
titulo: "Titulo del documento"
tipo: especificacion | guia | nota | decision
autor: "Carlos Canabal Cordero"
fecha_creacion: "YYYY-MM-DD"
fecha_modificacion: "YYYY-MM-DD"
version: "X.Y.Z"
estado: borrador | revision | publicado
tags: [tag1, tag2]
```

---

## Reglas de mantenimiento

1. **Un documento = un commit.** No mezclar cambios de
  diferentes documentos en el mismo commit.
2. **Changelog obligatorio.** Todo cambio significativo se
  registra en CHANGELOG.md.
3. **Revisar antes de merge.** Los documentos deben pasar
  revision antes de integrarse a main.
4. **Archivar, no borrar.** Los documentos obsoletos se
  mueven a `docs/archive/`, no se eliminan.

---

## Referencias

[1] ISO/IEC/IEEE 26514:2022. Systems and software engineering.

[2] AltexSoft. "Technical Documentation in Software Development".
    [https://www.altexsoft.com/blog/technical-documentation-in-](https://www.altexsoft.com/blog/technical-documentation-in-)
    software-development-types-best-practices-and-tools/

[3] MIT Broad Institute. "File Structure".
    [https://mitcommlab.mit.edu/broad/commkit/file-structure/](https://mitcommlab.mit.edu/broad/commkit/file-structure/)

[4] adr.github.io. "Architectural Decision Records".
    [https://adr.github.io/](https://adr.github.io/)

---

## Cambios


| Version | Fecha      | Cambios         |
| ------- | ---------- | --------------- |
| 0.1.0   | 2026-06-13 | Primer borrador |
| 0.2.0   | 2026-06-14 | Iter 6: estructura actualizada con `docs/specs/` que ahora existe. Carpetas reales: `standards/`, `decisions/` (8 ADRs), `research/` (7 docs), `specs/` (4 specs), `prompts/`, `references/`. |


---

*Estado: borrador*