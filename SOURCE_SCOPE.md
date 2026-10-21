# PDFStrike source scope

PDFStrike adapts pinned PdfCraft source at commit `b8ddf71e30959dc8a5ca4b66c40b8fcaea68cf76`. The development source archive SHA-256 was `f8767cc9cd42707a4683ec1c7cf556100a4d1d671076d9fc83ac9f7601e623e6`; that archive is not bundled. The root `vendor/pdfcraft` subset and the separate native workspace retain donor source under their original MIT/Apache license files and notices. The native workspace also retains original `ATTRIBUTION.md`/`ATTRIBUTION.toml`, Lucide and font licenses, and patched vendor crate notices.

The native `apps/pdfstrike-native` and `crates/pdfstrike-project`, and the root `apps/pdfstrike`, are adapted/new PDFStrike packages. Their candidate manifests make no first-party license assertion. The native and CLI project cores are separate, tested interoperable for the reviewed archive subset, and not byte-identical. Neither is silently replaced here.

Original donor notices mention brand, showcase, and optional OCR/font inputs omitted from this source checkout. They remain intact as upstream provenance, not claims that those assets or features are included. The distinct PDFStrike app icon, runtime Lucide assets, fonts, and their notices remain because the native build uses them. The signing test `.p12` files are public, synthetic donor fixtures with password `test`; their own README says they protect nothing, and they match the pinned donor archive.

Excluded: task-local build/target caches, source archive, captured media, private projects, OCR models, unrelated Office apps, and machine-specific evidence. The packaged fixture PDFs are self-authored synthetic tests. Distribution notices for a future binary and installer behavior require separate review.

## Redaction backports (2026-10-21)

The native `crates/redact` implementation additionally adapts these PdfCraft updates:

- [`d695bc56545944fe9c7181901c61d0bf1c11c887`](https://github.com/storytold/pdfcraft/commit/d695bc56545944fe9c7181901c61d0bf1c11c887): evaluate OCMD policies and bounded visibility expressions during hidden-layer sanitization, cache decisions, retain off-group state needed by surviving content, and count hidden/undecodable/depth-limited XObjects during verification.
- [`236ce9a064379854447fec59dcdeb61bcbea22bc`](https://github.com/storytold/pdfcraft/commit/236ce9a064379854447fec59dcdeb61bcbea22bc): clear every image sample whose transformed footprint intersects a redaction region, conservatively including rotated/skewed footprints.

The corresponding upstream regression tests are retained, with the image stream helper adapted to this older source and additional checks for saved payloads, shared/inherited resources, masks, packed samples, and malformed expressions. These are targeted backports, not a replacement of the pinned donor snapshot above. Original donor licenses and notices remain intact.

Both updates depend on removing stale original XObject resource aliases before a full save. This older snapshot lacked upstream's resource cleanup, so the port includes the required tracking and page/Form/shared-resource cleanup in the native redaction crate. A live draw or unaffected page keeps its original object. Cleanup fails when this interpreter cannot establish resource use, including potential inherited Type 3 glyph, pattern, or soft-mask users. The native engine's existing whole-document transaction and the separate PDFStrike wrappers/project cores are unchanged. No dependency versions change.

Review adaptations also resolve indirect OCMD type/operator values, remove unused Form-owned alias paths, and preserve persistent graphics state and text advances when suppressing hidden painting. The indirect-value handling follows [PDF syntax guidance, section 7.3.10](https://pdf-issues.pdfa.org/32000-2-2020/clause07.html); optional-content state handling follows [Adobe PDF Reference 1.6, section 4.10.2](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.6.pdf). Hidden clipping text and zero-size text with character/word spacing are refused when a safe state-preserving rewrite cannot be established.

Publication review additionally covers indirect XObject subtypes and bounded structural checks for original-object aliases in other resource containers, malformed XObject values, and surviving XObject metadata. Unsupported alias paths cause failure rather than a successful incomplete cleanup. ExtGState font selection is included in graphics-state tracking, including save/restore; hidden text with unestablished font advances (including indirect font subtypes or composite encodings beyond this metrics reader's Identity-H support) is refused.

## Optional patched-donor fixtures

The four PNGs under `native/pdfstrike/vendor/winit/examples/data/` were restored byte-for-byte from the locally cached **winit 0.30.13** crates.io source package. They are example-only, not PDFStrike runtime art. The cached package and this patched vendor copy carry the same Apache-2.0 `LICENSE`; their source SHA-256 values are recorded in the local candidate verification report. No example was compiled during source preparation.

The patched **lopdf 0.45.0** crate's own `src/reader.rs` unit test refers to `assets/example.pdf`. Its normalized crates.io `Cargo.toml` explicitly excludes `/assets`, and neither the local package cache nor the pinned PdfCraft source archive contains that file. No substitute PDF was invented. The native workspace explicitly excludes `vendor/`, so its supported `cargo test --workspace` and PDFStrike app targets do **not** compile or run lopdf's direct unit/integration tests or donor examples. Direct testing of the vendored lopdf package is unsupported here until the original fixture and its provenance are supplied. This does not claim the donor test passed.
