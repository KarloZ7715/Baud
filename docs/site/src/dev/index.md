# Development

This section describes how Baud is built: process and thread model, module ownership, hot paths (terminal, render, input), platform backends, multiplexing, the contributor loop, release packaging, and the security model behind self-update and diagnostics.

It is aimed at contributors and at maintainers returning to a subsystem after time away. User-facing feature docs live under [Features](../features/tabs-and-splits.md); this section stays at subsystem altitude — what owns what and why — not a walk-through of every function. For exact APIs, read the modules under `src/` or generate rustdoc locally.

## Start here

| If you need to… | Read |
| --- | --- |
| Place a behavior in the system | [Architecture](architecture.md), [Module map](modules.md) |
| Change parse, grid, or scrollback | [Terminal model](terminal-model.md) |
| Change GPU drawing or glyphs | [Rendering](rendering.md) |
| Change keys, bindings, or the wheel | [Input](input.md) |
| Fix a Windows-only or WSL issue | [Platform backends](platform.md) |
| Change tabs, splits, or focus | [Multiplexing](multiplexing.md) |
| Build, test, and open a first PR | [Building](building.md), [Testing](testing.md), [CI](ci.md), [Conventions](conventions.md) |
| Reproduce a release | [Release](release.md), [Packaging](packaging.md) |
| Audit updater or telemetry claims | [Security model](security-model.md), [Telemetry internals](telemetry-internals.md) |
| Match site voice and rules | [Documentation style](docs-style.md) |

## Ground rules for these pages

Every claim here must trace to current code under `src/`, `tools/`, `.github/workflows/`, or a manifest. Archived design notes may explain *why* something was tried; they never justify a sentence about *what* the code does today.

Pages describe structure and intent. Where you need the exact API, open the named module rather than expecting signatures here.
