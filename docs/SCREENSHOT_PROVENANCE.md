# PDFStrike screenshot provenance

## Hosted Windows capture on October 21, 2026

The README now uses the three native-window PNGs below. They are actual visible windows captured on a standard GitHub Windows runner, including the OS titlebar and geometric PDFStrike icon. The accompanying client PNGs come from the running application's `ui.screenshot` endpoint and exclude OS decorations. No UI elements were composited, redrawn or added to these files.

[Hosted run 39669070169](https://github.com/Gerbzz/PDFStrike/actions/runs/39669070169) built and captured exact source [`20deb626b079ca7d4c222c65f8854ba1ac74d7a6`](https://github.com/Gerbzz/PDFStrike/commit/20deb626b079ca7d4c222c65f8854ba1ac74d7a6). Its native source tree is `fe16d291dbb50458499b06ef9b06b9289cf8a3fd`, identical to published main `5818fb007312aec8c7bc3369f79b83c458ea8551`; the capture commit changes workflow/tooling only. The native executable SHA-256 is `e2c4c573bc42e9fe70e95a264ddc1042211096ab1cbc3dbb407b4a36feab6c09`.

Both jobs passed. Linux ran 57 redaction tests. Windows ran 57 redaction, 14 native project, 15 root CLI and one engine redaction transaction/undo test: 144 passing test executions, zero failures. Formatting, strict redaction library Clippy, native editor build, all three captures and artifact uploads also passed. Independent source and visual review confirmed the titlebar icon, project wrapper, saved fictional note and standalone Fields panel.

The hosted desktop work area was `1024 x 720`, DPI 96. The native-window captures are `1010 x 713`; the client captures are `1008 x 681`. Rendering used wgpu with the Microsoft Basic Render Driver (CPU, Direct3D 12/WARP). The helper copied the visible DWM frame bounds without resizing the resulting images. Captures occurred at 20:56:26 UTC (project overview), 20:56:30 (saved note) and 20:56:35 (standalone form), all on October 21, 2026.

| File under `assets/screenshots/current/` | SHA-256 |
| --- | --- |
| `pdfstrike-project-overview-current-window.png` | `7f8b64b4a7d9e343e6670a1137e06a915067aca0ec41dae18e2825be3fa9573c` |
| `pdfstrike-project-overview-current-client.png` | `420772e6d9bcde86f5c9b8d102e6d2323c93c3afe17ab8809a63154d2482eaf1` |
| `pdfstrike-review-note-current-window.png` | `2bbc577ca7d5aad74728b698d4b5581cfb8f1cae683b265ceb6e0d50cb8cd18a` |
| `pdfstrike-review-note-current-client.png` | `0a22dbf1b2a8196a942e5e28d5c2fa962081fd290dda44ce341eb5fc698dcbf4` |
| `pdfstrike-form-fields-current-window.png` | `f06b4af908d11f055d6911e7434d458cf9a520242573c0098babb5af71f68607` |
| `pdfstrike-form-fields-current-client.png` | `44bcb37cc690ab0840b4a78778b4b068b6181084c02a40f096ebb57646111957` |

The unchanged [`capture-manifest.json`](../assets/screenshots/current/capture-manifest.json) records timestamps, exact source/tree IDs, executable/helper/input/logo/image hashes, actual renderer, view state and the saved-note check. The capture helper used a fresh portable profile beside a staged development executable. Its project input was the checked-in self-authored `native-compat.pdfstrike` archive (SHA-256 `9fd1f8d7dc2245f2ba29d5b88d16854dd60b87d0629c5427bf5810e56d2a18be`). The note text was `Synthetic review note: confirm the packet order before export.`; the helper verified it in the saved archive at journal cursor 2, and the screenshot shows its yellow page marker. The form input was the self-authored `form-real.pdf` (SHA-256 `b722d6e3facb821511e95146aabf6f8b704337a51aa0cd9c6affd42da06e7a20`), opened as a standalone PDF. That capture shows two listed fields, `contact.name` and `approved`; it does not demonstrate a project Forms edit or a filled field.

The manifest also records the unused reference `packet-real.pdf`. Its Git blob is 3,542 bytes with SHA-256 `0b6c3b8a40d5b388ef31581aa88d8930ae1573621af69c394296c91e7a0ceaa6`. Git's Windows checkout converts its LF line endings to CRLF, producing 3,715 bytes and SHA-256 `10121192eaacea0a687519d4e49dc3629d677c70ef1b5de6ed2130a27913f887`, exactly the file hash recorded during capture. This file was not opened by the capture helper. Both actual opened inputs retain the documented hashes above. The lightweight provenance job runs on Windows to reproduce the recorded checkout bytes. It also accepts the exact LF/CRLF variants of the unchanged helper source: reviewed LF hash `242419d94c6037e64fab404fbdc69c94a3ab32e960428816171e0578b20130c7`, executed Windows CRLF hash `3b314131d9de113b022746c39ea18abf436cbfa30c2021e850c70cfd039e489e`.

The run retained current UI artifact `12381360417` (471,284-byte ZIP, SHA-256 `2143ac9fcf73398df9a5ffdd485d96818c90204905f59c485690851f06791dd5`) and development executable/provenance artifact `12380679643` (27,739,725-byte ZIP). Their retention expires November 4, 2026; the checked-in PNGs and manifest preserve the capture evidence afterward. Artifact retention is a development build record, not a release or deployment.

These captures establish the displayed synthetic workflows on the hosted Windows runner. They do not establish Room interactive acceptance, installer/package behavior, broad PDF compatibility, arbitrary Forms/annotation persistence or hostile-PDF isolation. No Room native executable or previously denied diagnostic was retried for this update. The historical images and their provenance remain below and are retained unchanged in the repository.

## Historical October 14 captures

The three README images are screenshots from a live PDFStrike native Windows window at 1440 × 920. They were captured through the application's localhost `ui.screenshot` endpoint on 2026-10-14 UTC. No UI elements were composited into the images.

| Repository path | SHA-256 | What the window showed |
| --- | --- | --- |
| `assets/screenshots/pdfstrike-project-overview.png` | `69AE3D4E67B6AF5836949239652D5641415D2C15A046B0DC830FFC5E642B0A3E` | Page one fitted in the native project editor |
| `assets/screenshots/pdfstrike-review-note.png` | `3FE8AD4788F74EB1509855250469CB8028B1303889529FD2B70AA996621A395A` | Saved review note and its page marker |
| `assets/screenshots/pdfstrike-form-field.png` | `2B51FBB1F6B2C9AEC7661F2A3087412F687F130E941B7DABB4B81BCE07F5D5BF` | Page three with a populated `reviewer.name` text field |

The sample PDF, **North Cove / Field Notes**, was authored for this capture using ReportLab. It has three original pages and two demonstration AcroForm fields. The reviewer name is fictional; the packet contains no client material or external images. A root CLI created its `.pdfstrike` project, then the retained native editor opened it in an isolated portable profile. The editor saved a manual-coordinate note and `reviewer.name = Avery Chen`. The third screenshot shows the form value in the page preview. The sample PDF and editable project are capture fixtures, not distributed with this source repository.

The retained `pdfstrike-native.exe` used for this capture is the independently reviewed 2026-10-11 portable checkpoint (`E31A59E1A23F115EB60A3B42A926C553AED11CB000157D6E0D7552BDCDF8EB50` SHA-256). A newer retained baseline binary had 591 source inputs matched to the public repository, but it was not used for screenshots because it lacked the isolated portable profile. The capture does not establish that all later source revisions or menu commands work. No build or full test suite was run for this README update.
