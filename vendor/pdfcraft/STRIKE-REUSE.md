# Pinned PdfCraft source subset for PDFStrike

This directory contains unmodified Rust crates copied from `storytold/pdfcraft` commit `b8ddf71e30959dc8a5ca4b66c40b8fcaea68cf76`, plus the source-required `DancingScript.ttf` under its bundled OFL-1.1 license. The workspace manifest was added by Strike to list only selected crates; Strike-specific code is outside this directory. Original `LICENSE-MIT`, `LICENSE-APACHE`, and `NOTICE` are retained. See the repository-root `SOURCE_SCOPE.md` for exact source boundaries, exclusions, and remaining distribution gates.

No ArtCraft logos, brand assets, upstream application binaries, OCR models, sample PDFs, or hosted-service code are included here. This source subset is not yet a released dependency bundle; transitive redistribution notices and runtime safety still need final review.

Bundled font: `assets/fonts/DancingScript.ttf`, Copyright 2016 The Dancing Script Project Authors, OFL-1.1, SHA-256 `21808625578fe8d8cd10cb684be546dca077b27cd03a53a2f1ec11dc743c924c`. Original source and attribution are in PdfCraft's pinned `ATTRIBUTION.toml`; the OFL text is retained beside the font.
