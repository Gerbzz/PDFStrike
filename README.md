# PDFStrike

**by PWDS**

**Work through a PDF your way.**

PDFStrike provides an editable project for focused changes to a PDF. Move, rotate, or remove pages; add notes and highlights; edit supported text and checkbox fields; then export to a new PDF.

### Highlights

- Reorder and adjust pages in a project.
- Review with notes and highlights.
- Save edits and export a new PDF.

## Current status

Windows alpha source. The root and native project cores passed bounded tests, and the native editor built; one rotation workflow was exercised across UI and CLI. Hostile-PDF process isolation, broad annotation and signed-file behavior, accessibility, and release packaging are not yet validated.

## Edit a PDF

Open a PDF into a project, make supported page or annotation changes, and save the project so you can continue later. Export writes a new PDF rather than overwriting the original. Signed files and hostile inputs require more validation before general use.

## Source layout, build, and verification

This source checkout contains two existing Rust workspaces:

- The root `apps/pdfstrike` command-line project/core, with its pinned PdfCraft subset in `vendor/pdfcraft`.
- `native/pdfstrike`, the adapted native editor and `pdfstrike-project` core. It retains its own `Cargo.lock`, PdfCraft source graph, patched vendor crates, and source-required assets.

Keep the directory layout intact: both project-core test suites refer to the self-authored PDFs and archive under `fixtures/pdfstrike/`. The two cores share the `.pdfstrike` archive format but have separate source revisions. No untested dependency consolidation was made.

On Windows, after ordinary crates.io dependencies are available, the root CLI can be checked with `cargo test --locked -p pdfstrike`. From `native/pdfstrike`, use `cargo test --locked -p pdfstrike-project` and `cargo build --locked -p pdfstrike-native --bin pdfstrike-native`. The clean standalone source passed 15 root tests and 14 native project tests, including CLI archive compatibility, and built the native editor with required Windows resource compilation. These documentation updates do not change a build input. The native workspace excludes patched donor crates as direct test targets; vendored lopdf's own optional tests still need its unavailable original `assets/example.pdf` fixture.

The independently reviewed native subset supports page move/rotate/delete, note/highlight, text/checkbox AcroForm edits, project save/reopen/undo/redo, and a new non-overwriting PDF export. The Windows UI was exercised for a project rotation, and the CLI reopened the resulting archive. Hostile-PDF process isolation, broad annotation persistence, real signed-file behavior, power-loss recovery, accessibility, and non-Windows platforms are not validated. No installer is supported by this source checkpoint.

See [SOURCE_SCOPE.md](SOURCE_SCOPE.md) for ownership and donor notices. No first-party license grant is stated for PDFStrike here.
