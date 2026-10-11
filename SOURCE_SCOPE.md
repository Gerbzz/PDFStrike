# PDFStrike source scope

PDFStrike adapts pinned PdfCraft source at commit `b8ddf71e30959dc8a5ca4b66c40b8fcaea68cf76`. The development source archive SHA-256 was `f8767cc9cd42707a4683ec1c7cf556100a4d1d671076d9fc83ac9f7601e623e6`; that archive is not bundled. The root `vendor/pdfcraft` subset and the separate native workspace retain donor source under their original MIT/Apache license files and notices. The native workspace also retains original `ATTRIBUTION.md`/`ATTRIBUTION.toml`, Lucide and font licenses, and patched vendor crate notices.

The native `apps/pdfstrike-native` and `crates/pdfstrike-project`, and the root `apps/pdfstrike`, are adapted/new PDFStrike packages. Their candidate manifests make no first-party license assertion. The native and CLI project cores are separate, tested interoperable for the reviewed archive subset, and not byte-identical. Neither is silently replaced here.

Original donor notices mention brand, showcase, and optional OCR/font inputs omitted from this source checkout. They remain intact as upstream provenance, not claims that those assets or features are included. The distinct PDFStrike app icon, runtime Lucide assets, fonts, and their notices remain because the native build uses them. The signing test `.p12` files are public, synthetic donor fixtures with password `test`; their own README says they protect nothing, and they match the pinned donor archive.

Excluded: task-local build/target caches, source archive, captured media, private projects, OCR models, unrelated Office apps, and machine-specific evidence. The packaged fixture PDFs are self-authored synthetic tests. Distribution notices for a future binary and installer behavior require separate review.

## Optional patched-donor fixtures

The four PNGs under `native/pdfstrike/vendor/winit/examples/data/` were restored byte-for-byte from the locally cached **winit 0.30.13** crates.io source package. They are example-only, not PDFStrike runtime art. The cached package and this patched vendor copy carry the same Apache-2.0 `LICENSE`; their source SHA-256 values are recorded in the local candidate verification report. No example was compiled during source preparation.

The patched **lopdf 0.45.0** crate's own `src/reader.rs` unit test refers to `assets/example.pdf`. Its normalized crates.io `Cargo.toml` explicitly excludes `/assets`, and neither the local package cache nor the pinned PdfCraft source archive contains that file. No substitute PDF was invented. The native workspace explicitly excludes `vendor/`, so its supported `cargo test --workspace` and PDFStrike app targets do **not** compile or run lopdf's direct unit/integration tests or donor examples. Direct testing of the vendored lopdf package is unsupported here until the original fixture and its provenance are supplied. This does not claim the donor test passed.
