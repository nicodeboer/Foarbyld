# ADR 0003: Project license

- Status: Proposed
- Date: 2026-11-12

## Context

The project is open source and cross-platform. The license must be compatible
with our dependencies (Tauri: MIT/Apache-2.0, PDFium: BSD-3-Clause/Apache-2.0,
`pdfium-render`: MIT/Apache-2.0) and should lower the barrier for
contributors and downstream packagers (Flathub, winget, Linux distributions).

## Decision

License the project under **Apache-2.0**.

Contributions are accepted under the same license, using the Developer
Certificate of Origin (`Signed-off-by` in commits) instead of a CLA.

## Alternatives considered

- **MIT**: simpler, but has no explicit patent grant.
- **GPL-3.0 / AGPL-3.0**: stronger copyleft, keeps all forks open. Required if
  we choose MuPDF (see ADR 0002). Rejected for now because it limits reuse and
  adoption and nothing in the current stack requires it.
- **MPL-2.0**: file-level copyleft. A reasonable middle ground; not chosen
  because the added complexity is not needed at this stage.

## Consequences

- Anyone can use, modify and redistribute the app, including in closed-source
  products. We accept that.
- Dependencies must stay Apache-2.0-compatible. Add a CI check (for example
  `cargo deny` for Rust and a license checker for npm) to block GPL/AGPL
  dependencies.
- Keep a `NOTICE`/third-party licenses file in releases, including PDFium.
- Choosing a different engine (MuPDF) or copyleft later means superseding
  this ADR and relicensing, which gets harder as contributors accumulate.
