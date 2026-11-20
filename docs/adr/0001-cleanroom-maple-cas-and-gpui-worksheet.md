# ADR 0001: Cleanroom Maple-Inspired CAS & GPUI Interactive Worksheet

- **Status**: Accepted
- **Date**: 2026-11-21
- **Context**: UNSW Mathematics coursework (e.g. MATH1131/1231) relies on symbolic algebra, calculus, linear algebra, and 2D plotting via Maple on Citrix Workspace. Running heavy remote Java/X11 sessions is slow offline or over remote links. We want a fast, lightweight native Rust desktop worksheet using `gpui 0.2.2` implemented cleanly from mathematical first principles.

## Decision

1. **Cleanroom CAS Kernel**: Implement a self-contained symbolic engine in Rust without calling external CAS processes or copying proprietary code. Use exact `i128` `Rational` arithmetic for constants and rational coefficients so exact reductions (RREF, inverses, Taylor coefficients, definite integrals) never lose precision.
2. **Maple-Compatible Surface Syntax**: Support core Maple worksheet ergonomics: `:=` assignment, `;` vs `:` output suppression, `%` and `%%` history registers, `x -> expr` arrow functions, `a..b` ranges, `diff(f, x)` / `diff(f, x$n)`, `int`, `limit`, `series`/`taylor`, `solve`, `expand`, `factor`, `subs`, `evalf`, `Matrix`, `det`, `Transpose`, `Inverse`, `rref`, `trace`, `Eigenvalues`, and `plot`.
3. **GPU-Accelerated Native UI (`gpui 0.2.2`)**: Build a modernised Maple-layout worksheet UI with a menu strip and execution ribbon (`!!!`, `!`, `|>`, `Restart Kernel`), a tabbed `Palettes`/`Workbook` left dock (Expression, Calculus, live `Variable | Value` table, Matrix template builder, and Greek/common symbols), multi-tab `Start.mw` + `.mw` worksheet canvas with `[>` execution groups, bracketed matrix grids, and inline `gpui::canvas` vector plots, a collapsible Context Panel + Math Editor Shortcuts right dock, and a bottom status bar.
4. **Headless CLI Mode (`--eval`)**: Expose a deterministic headless runner (`maple_syrup --eval "..."`) sharing the exact same `Worksheet` domain layer for automated verification and quick terminal calculations.

## Consequences

- Zero runtime dependencies beyond standard Wayland/X11 graphics libraries provided in the Nix `devShell`.
- Sub-millisecond evaluation for typical undergraduate calculus and exact rational matrix workloads.
- Unimplemented advanced Maple packages (e.g., proprietary `Student[Calculus1]` interactive tutors or high-degree symbolic Groebner bases) return explicit diagnostic errors rather than silently approximating.
