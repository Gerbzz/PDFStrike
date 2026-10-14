# PDFStrike screenshot provenance

The three README images are screenshots from a live PDFStrike native Windows window at 1440 × 920. They were captured through the application's localhost `ui.screenshot` endpoint on 2026-10-14 UTC. No UI elements were composited into the images.

| Repository path | SHA-256 | What the window showed |
| --- | --- | --- |
| `assets/screenshots/pdfstrike-project-overview.png` | `69AE3D4E67B6AF5836949239652D5641415D2C15A046B0DC830FFC5E642B0A3E` | Page one fitted in the native project editor |
| `assets/screenshots/pdfstrike-review-note.png` | `3FE8AD4788F74EB1509855250469CB8028B1303889529FD2B70AA996621A395A` | Saved review note and its page marker |
| `assets/screenshots/pdfstrike-form-field.png` | `2B51FBB1F6B2C9AEC7661F2A3087412F687F130E941B7DABB4B81BCE07F5D5BF` | Page three with a populated `reviewer.name` text field |

The sample PDF, **North Cove / Field Notes**, was authored for this capture using ReportLab. It has three original pages and two demonstration AcroForm fields. The reviewer name is fictional; the packet contains no client material or external images. A root CLI created its `.pdfstrike` project, then the retained native editor opened it in an isolated portable profile. The editor saved a manual-coordinate note and `reviewer.name = Avery Chen`. The third screenshot shows the form value in the page preview. The sample PDF and editable project are capture fixtures, not distributed with this source repository.

The retained `pdfstrike-native.exe` used for this capture is the independently reviewed 2026-10-11 portable checkpoint (`E31A59E1A23F115EB60A3B42A926C553AED11CB000157D6E0D7552BDCDF8EB50` SHA-256). A newer retained baseline binary had 591 source inputs matched to the public repository, but it was not used for screenshots because it lacked the isolated portable profile. The capture does not establish that all later source revisions or menu commands work. No build or full test suite was run for this README update.
