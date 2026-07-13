# Changelog

All user-facing changes are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and Baud follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.7](https://github.com/KarloZ7715/Baud/compare/v0.0.6...v0.0.7) - 2026-07-13

### Added

- *(diagnostics)* añadir config diagnostics/reporting + consent state machine + persist toml
- *(diagnostics)* embeber DSN por defecto del proyecto en el binario
- *(install)* registrar desktop entry e íconos XDG desde el tarball verificado
- *(cli)* add command dispatcher for version, help and update
- *(install)* write and validate official-install ownership receipt
- *(updater)* implement verified self-update with signed manifest

### Fixed

- *(release)* publicar borrador sin checkout
- *(diagnostics)* traducir texto user-facing a inglés y corregir bugs de review
- *(diagnostics)* corregir formato de timestamp a ISO 8601 requerido por Sentry
- *(install)* eliminar código muerto y usar awk para Exp y evitar bug con sed
- *(ci)* restaurar jobs originales y agregar shell-fixtures sin borrar nada
- *(ci)* instalar desktop-file-utils antes de validar desktop entry
- *(updater)* verifica manifiesto y limpia staging
- *(updater)* desacopla version instalada en tests

### Packaging

- *(packaging)* incluir desktop entry e íconos en el tarball de release
- *(packaging)* crea target para signer

## [0.0.6] - 2026-07-12

This is the first experimental Baud release for Linux x86_64.

### Packaging

- Publish AppImage, deb, rpm, and tarball assets with a SHA-256 manifest.
- Provide a checksum-verified installer for the Linux x86_64 tarball.

### Added

- Distribute Baud through normal GitHub Releases while it remains pre-1.0 software.

### Compatibility

- Windows and macOS are not supported platforms yet.
