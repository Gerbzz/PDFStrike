use std::sync::Arc;

use pdfcraft_annot::{Meta, NewAnnotation, Shape, Style, add_annotation, rect_quad};
use pdfcraft_cos::{Document, SaveOptions, write_full, write_incremental};

use super::*;

// Ported from PdfCraft d695bc56545944fe9c7181901c61d0bf1c11c887 and
// 236ce9a064379854447fec59dcdeb61bcbea22bc; see SOURCE_SCOPE.md.
fn named_image(doc: &Document, page: usize, name: &[u8]) -> Option<Vec<u8>> {
    let page = &pdfcraft_model::pages(doc)[page];
    let resources = doc.resolve(page.dict.get(b"Resources")?);
    let xobjects = doc.resolve(resources.as_dict()?.get(b"XObject")?);
    let image = doc.resolve(xobjects.as_dict()?.get(name)?);
    match &*image {
        Object::Stream(s) => s.decoded().ok(),
        _ => None,
    }
}

fn assert_no_stream_payload(doc: &Document, payload: &[u8]) {
    for number in doc.object_numbers() {
        if let Object::Stream(s) = &*doc.get(ObjRef::new(number, doc.generation(number))) {
            let decoded = s.decoded().unwrap();
            assert!(!decoded.windows(payload.len()).any(|w| w == payload), "payload survived in stream {number}");
        }
    }
}

#[test]
fn hidden_ocmd_all_on_content_is_removed_before_its_off_group_is_unlinked() {
    use crate::sanitize::{Hidden, remove_hidden};

    let mut doc = one_page(
        b"BT /F1 10 Tf 10 200 Td (PUBLIC) Tj ET /OC /M BDC BT /F1 10 Tf 10 100 Td (SECRET) Tj ET EMC",
        "/Properties << /M 9 0 R >>",
        vec![
            b"<< /Type /OCG /Name (On) >>".to_vec(),
            b"<< /Type /OCG /Name (Off) >>".to_vec(),
            b"<< /Type /OCMD /OCGs [7 0 R 8 0 R] /P /AllOn >>".to_vec(),
        ],
    );
    let root = doc.root().unwrap();
    doc.update_dict(root, |d| {
        d.set(
            b"OCProperties".to_vec(),
            Object::Dict(pdfcraft_cos::Dict::from_iter([
                (b"OCGs".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))])),
                (
                    b"D".to_vec(),
                    Object::Dict(pdfcraft_cos::Dict::from_iter([
                        (b"Order".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))])),
                        (b"OFF".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(8, 0))])),
                    ])),
                ),
            ])),
        );
    })
    .unwrap();

    let before = pdfcraft_edit::text_lines(&doc, 0).unwrap();
    assert!(before.iter().any(|line| line.text.contains("PUBLIC")));
    assert!(before.iter().any(|line| line.text.contains("SECRET")), "extractor sees source text before sanitization");
    remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();

    let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
    let lines = pdfcraft_edit::text_lines(&saved, 0).unwrap();
    assert!(lines.iter().any(|line| line.text.contains("PUBLIC")), "visible content remains: {lines:?}");
    assert!(!lines.iter().any(|line| line.text.contains("SECRET")), "hidden text is extractable: {lines:?}");
    assert!(!write_full(&saved, &SaveOptions::default()).unwrap().windows(6).any(|w| w == b"SECRET"));
}

#[test]
fn hidden_ocmd_content_in_a_form_and_nested_marked_blocks_is_removed() {
    use crate::sanitize::{Hidden, remove_hidden};

    let mut doc = one_page(
        b"/Fm Do",
        "/Properties << /M 9 0 R >> /XObject << /Fm 10 0 R >>",
        vec![
            b"<< /Type /OCG /Name (On) >>".to_vec(),
            b"<< /Type /OCG /Name (Off) >>".to_vec(),
            b"<< /Type /OCMD /OCGs [7 0 R 8 0 R] /P /AllOn >>".to_vec(),
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> /Properties << /M 9 0 R >> >>",
                b"/OC /M BDC /OC /M BDC BT /F1 10 Tf 10 100 Td (SECRET) Tj ET EMC EMC",
            ),
        ],
    );
    let root = doc.root().unwrap();
    doc.update_dict(root, |d| {
        d.set(
            b"OCProperties".to_vec(),
            Object::Dict(Dict::from_iter([
                (b"OCGs".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))])),
                (b"D".to_vec(), Object::Dict(Dict::from_iter([(b"OFF".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(8, 0))]))]))),
            ])),
        );
    })
    .unwrap();

    remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
    let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
    assert!(!pdfcraft_edit::text_lines(&saved, 0).unwrap().iter().any(|line| line.text.contains("SECRET")));
    assert_no_stream_payload(&saved, b"SECRET");
}

#[test]
fn hidden_ocmd_alloff_is_sanitized_when_every_group_is_on() {
    use crate::sanitize::{Hidden, remove_hidden, scan};

    let mut doc = one_page(
        b"/OC /M BDC BT /F1 10 Tf 10 100 Td (SECRET) Tj ET EMC",
        "/Properties << /M 9 0 R >>",
        vec![
            b"<< /Type /OCG /Name (On) >>".to_vec(),
            b"<< /Type /OCG /Name (Unused) >>".to_vec(),
            b"<< /Type /OCMD /OCGs [7 0 R] /P /AllOff >>".to_vec(),
        ],
    );
    let counts = scan(&doc);
    assert!(counts.iter().any(|(h, n)| *h == Hidden::HiddenLayers && *n > 0), "{counts:?}");
    remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();

    let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
    assert!(!pdfcraft_edit::text_lines(&saved, 0).unwrap().iter().any(|line| line.text.contains("SECRET")));
}

#[test]
fn hidden_ve_not_is_sanitized_when_its_group_is_on() {
    use crate::sanitize::{Hidden, remove_hidden, scan};

    let mut doc = one_page(
        b"/OC /M BDC BT /F1 10 Tf 10 100 Td (SECRET) Tj ET EMC",
        "/Properties << /M 9 0 R >>",
        vec![b"<< /Type /OCG /Name (On) >>".to_vec(), b"<< /Type /OCG /Name (Unused) >>".to_vec(), b"<< /Type /OCMD /VE [/Not 7 0 R] >>".to_vec()],
    );
    let counts = scan(&doc);
    assert!(counts.iter().any(|(h, n)| *h == Hidden::HiddenLayers && *n > 0), "{counts:?}");
    remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();

    let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
    assert!(!pdfcraft_edit::text_lines(&saved, 0).unwrap().iter().any(|line| line.text.contains("SECRET")));
}

fn all_on_ocmd_xobject(subtype: &str, policy: &str) -> Document {
    let membership = match policy {
        "AllOff" => "<< /Type /OCMD /OCGs [7 0 R] /P /AllOff >>",
        "Not" => "<< /Type /OCMD /VE [/Not 7 0 R] >>",
        _ => "<< /Type /OCMD /OCGs [7 0 R] /P /AllOff >>",
    };
    let (object_number, oc) = if policy == "DirectAllOff" { (8, membership) } else { (9, "8 0 R") };
    let xobject = match subtype {
        "Image" => stream(
            &format!("/Type /XObject /Subtype /Image /Width 4 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /OC {oc}"),
            b"PRIVATE_HIDDEN_XOBJECT_IMAGE",
        ),
        _ => stream(
            &format!("/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> >> /OC {oc}"),
            b"BT /F1 10 Tf 10 100 Td (PRIVATE_HIDDEN_XOBJECT_FORM) Tj ET",
        ),
    };
    let mut extras = vec![b"<< /Type /OCG /Name (On) >>".to_vec()];
    if object_number == 9 {
        extras.push(membership.as_bytes().to_vec());
    }
    extras.push(xobject);
    let mut doc = one_page(b"/X Do", &format!("/XObject << /X {object_number} 0 R >>"), extras);
    let root = doc.root().unwrap();
    doc.update_dict(root, |d| {
        d.set(
            b"OCProperties".to_vec(),
            Object::Dict(Dict::from_iter([
                (b"OCGs".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0))])),
                (b"D".to_vec(), Object::Dict(Dict::from_iter([(b"BaseState".to_vec(), Object::name("ON"))]))),
            ])),
        );
    })
    .unwrap();
    doc
}

#[test]
fn hidden_all_on_ocmd_image_and_form_xobjects_are_counted_and_removed() {
    use crate::sanitize::{Hidden, remove_hidden, scan};

    let mut failures = Vec::new();
    for subtype in ["Image", "Form"] {
        for policy in ["AllOff", "Not"] {
            let mut doc = all_on_ocmd_xobject(subtype, policy);
            let before = scan(&doc);
            let hidden_count = before.iter().find(|(h, _)| *h == Hidden::HiddenLayers).map_or(0, |(_, n)| *n);
            if hidden_count == 0 {
                failures.push(format!("{subtype} /{policy} was not counted: {before:?}"));
            }

            match remove_hidden(&mut doc, &[Hidden::HiddenLayers]) {
                Ok(removed) if removed.iter().any(|(h, n)| *h == Hidden::HiddenLayers && *n > 0) => {}
                Ok(removed) => failures.push(format!("{subtype} /{policy} remove report is empty: {removed:?}")),
                Err(error) => failures.push(format!("{subtype} /{policy} remove failed: {error}")),
            }
            let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
            if bytes.windows(b"PRIVATE_HIDDEN_XOBJECT_IMAGE".len()).any(|w| w == b"PRIVATE_HIDDEN_XOBJECT_IMAGE") {
                failures.push(format!("{subtype} /{policy} image payload remains in full save"));
            }
            if bytes.windows(b"PRIVATE_HIDDEN_XOBJECT_FORM".len()).any(|w| w == b"PRIVATE_HIDDEN_XOBJECT_FORM") {
                failures.push(format!("{subtype} /{policy} Form payload remains in full save"));
            }
            let saved = Document::open(Arc::new(bytes)).unwrap();
            if pdfcraft_edit::text_lines(&saved, 0).unwrap().iter().any(|line| line.text.contains("PRIVATE_HIDDEN_XOBJECT_FORM")) {
                failures.push(format!("{subtype} /{policy} Form text remains extractable"));
            }
            if !pdfcraft_edit::page_images(&saved, 0).unwrap().is_empty() {
                failures.push(format!("{subtype} /{policy} image remains on the page"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn repeated_image_with_direct_alloff_membership_is_counted_per_draw() {
    use crate::sanitize::{Hidden, scan};

    let mut doc = all_on_ocmd_xobject("Image", "DirectAllOff");
    let page = pdfcraft_model::pages(&doc)[0].obj;
    let contents = doc.add(Object::Stream(pdfcraft_cos::Stream::flate(Dict::new(), b"/X Do /X Do /X Do")));
    doc.update_dict(page, |d| d.set(b"Contents".to_vec(), Object::Ref(contents))).unwrap();

    let original = write_full(&doc, &SaveOptions::default()).unwrap();
    let counts = scan(&doc);
    assert_eq!(counts.iter().find(|(h, _)| *h == Hidden::HiddenLayers).map(|(_, n)| *n), Some(3), "{counts:?}");
    assert_eq!(write_full(&doc, &SaveOptions::default()).unwrap(), original, "Verify scan must not mutate the document");
}

#[test]
fn undecodable_form_is_counted_and_removed_by_hidden_layer_sanitization() {
    use crate::sanitize::{Hidden, remove_hidden, scan};

    let mut doc = one_page(
        b"/Bad Do",
        "/XObject << /Bad 7 0 R >>",
        vec![stream("/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Filter /UnknownFilter", b"PRIVATE_UNDECODABLE_FORM")],
    );
    let counts = scan(&doc);
    assert!(counts.iter().any(|(h, n)| *h == Hidden::HiddenLayers && *n > 0), "{counts:?}");
    remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();

    let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
    assert!(!bytes.windows(b"PRIVATE_UNDECODABLE_FORM".len()).any(|w| w == b"PRIVATE_UNDECODABLE_FORM"));
}

#[test]
fn form_chain_at_max_depth_is_counted_and_removed_by_hidden_layer_sanitization() {
    use crate::sanitize::{Hidden, remove_hidden, scan};

    const PRIVATE: &[u8] = b"PRIVATE_FORM_BEYOND_MAX_DEPTH";
    // MAX_DEPTH is 12. The final Form is encountered at that depth and is removed
    // conservatively without being entered. Verify must count that removal or sanitize
    // can skip the HiddenLayers pass entirely.
    let mut forms = Vec::new();
    for depth in 0..=12 {
        let content = if depth == 12 {
            b"BT /F1 10 Tf 10 100 Td (PRIVATE_FORM_BEYOND_MAX_DEPTH) Tj ET".to_vec()
        } else {
            format!("/F{} Do", depth + 1).into_bytes()
        };
        forms.push(stream("/Type /XObject /Subtype /Form /BBox [0 0 300 300]", &content));
    }
    let xobjects = (0..=12).map(|depth| format!("/F{depth} {} 0 R", 7 + depth)).collect::<Vec<_>>().join(" ");
    let mut doc = one_page(b"/F0 Do", &format!("/XObject << {xobjects} >>"), forms);
    assert!(
        write_full(&doc, &SaveOptions::default()).unwrap().windows(PRIVATE.len()).any(|w| w == PRIVATE),
        "fixture lacked nested payload before sanitization"
    );

    let counts = scan(&doc);
    assert!(counts.iter().any(|(hidden, count)| *hidden == Hidden::HiddenLayers && *count > 0), "depth-limited Form was not counted: {counts:?}");
    let removed = remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
    assert!(
        removed.iter().any(|(hidden, count)| *hidden == Hidden::HiddenLayers && *count > 0),
        "depth-limited Form was not reported removed: {removed:?}"
    );
    let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
    assert!(!bytes.windows(PRIVATE.len()).any(|w| w == PRIVATE), "depth-limited Form payload remains in full save");
}

#[test]
fn hidden_ocmd_resolves_an_indirect_policy_name() {
    use crate::sanitize::{Hidden, remove_hidden};

    let mut doc = one_page(
        b"/OC /M BDC BT /F1 10 Tf 10 100 Td (VISIBLE) Tj ET EMC",
        "/Properties << /M 9 0 R >>",
        vec![
            b"<< /Type /OCG /Name (On) >>".to_vec(),
            b"<< /Type /OCG /Name (Off) >>".to_vec(),
            b"<< /Type /OCMD /OCGs [7 0 R 8 0 R] /P 12 0 R >>".to_vec(),
        ],
    );
    doc.set(ObjRef::new(12, 0), Object::name("AnyOn"));
    let root = doc.root().unwrap();
    doc.update_dict(root, |d| {
        d.set(
            b"OCProperties".to_vec(),
            Object::Dict(Dict::from_iter([
                (b"OCGs".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))])),
                (b"D".to_vec(), Object::Dict(Dict::from_iter([(b"OFF".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(8, 0))]))]))),
            ])),
        );
    })
    .unwrap();

    remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
    let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
    assert!(pdfcraft_edit::text_lines(&saved, 0).unwrap().iter().any(|line| line.text.contains("VISIBLE")));
}

#[test]
fn surviving_anyoff_and_not_content_keeps_its_off_group_state() {
    use crate::sanitize::{Hidden, remove_hidden};

    for use_not in [false, true] {
        let mut doc = one_page(
            b"/OC /M BDC BT /F1 10 Tf 10 100 Td (VISIBLE) Tj ET EMC",
            "/Properties << /M 9 0 R >>",
            vec![
                b"<< /Type /OCG /Name (On) >>".to_vec(),
                b"<< /Type /OCG /Name (Off) >>".to_vec(),
                if use_not { b"<< /Type /OCMD /VE [/Not 8 0 R] >>".to_vec() } else { b"<< /Type /OCMD /OCGs [8 0 R] /P /AnyOff >>".to_vec() },
            ],
        );
        let root = doc.root().unwrap();
        doc.update_dict(root, |d| {
            d.set(
                b"OCProperties".to_vec(),
                Object::Dict(Dict::from_iter([
                    (b"OCGs".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))])),
                    (b"D".to_vec(), Object::Dict(Dict::from_iter([(b"OFF".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(8, 0))]))]))),
                ])),
            );
        })
        .unwrap();

        remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
        let root = doc.root().unwrap();
        let catalog = doc.get(root);
        let oc = catalog.as_dict().and_then(|d| d.get(b"OCProperties")).map(|o| doc.resolve(o));
        let default = oc.as_deref().and_then(|o| o.as_dict()).and_then(|d| d.get(b"D")).map(|o| doc.resolve(o));
        let off = default.as_deref().and_then(|o| o.as_dict()).and_then(|d| d.get(b"OFF")).map(|o| doc.resolve(o));
        assert!(
            off.as_deref().and_then(Object::as_array).is_some_and(|a| a.iter().any(|o| o.as_ref() == Some(ObjRef::new(8, 0)))),
            "off group lost for /VE Not={use_not}"
        );
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        assert!(pdfcraft_edit::text_lines(&saved, 0).unwrap().iter().any(|line| line.text.contains("VISIBLE")));
    }
}

#[test]
fn hidden_ocmd_policy_matches_the_default_configuration() {
    use crate::sanitize::{Hidden, remove_hidden};

    let cases = [
        (None, false, None, false),
        (Some("AnyOn"), false, None, false),
        (Some("AllOn"), false, None, true),
        (Some("AnyOff"), false, None, false),
        (Some("AllOff"), false, None, true),
        (Some("UnknownPolicy"), false, None, true),
        (None, true, None, true),
        (None, false, Some("and"), true),
        (None, false, Some("and_single_not"), true),
        (None, false, Some("and_single_off"), true),
        (None, false, Some("or_single_on"), false),
        (None, false, Some("or_single_off"), true),
        (Some("AllOn"), false, Some("not_off"), false),
        (None, false, Some("malformed_not"), true),
        (None, false, Some("malformed_without_ocgs"), true),
    ];
    for (policy, malformed_policy, visibility, should_remove) in cases {
        let mut doc = one_page(
            b"/OC /M BDC BT /F1 10 Tf 10 100 Td (SECRET) Tj ET EMC",
            "/Properties << /M 9 0 R >>",
            vec![
                b"<< /Type /OCG /Name (On) >>".to_vec(),
                b"<< /Type /OCG /Name (Off) >>".to_vec(),
                b"<< /Type /OCMD /OCGs [7 0 R 8 0 R] /P /AllOn >>".to_vec(),
            ],
        );
        let mut membership = Dict::new();
        membership.set(b"Type".to_vec(), Object::name("OCMD"));
        if visibility != Some("malformed_without_ocgs") {
            membership.set(b"OCGs".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))]));
        }
        if let Some(policy) = policy {
            membership.set(b"P".to_vec(), Object::name(policy));
        } else if malformed_policy {
            membership.set(b"P".to_vec(), Object::Int(17));
        }
        match visibility {
            Some("and") => membership
                .set(b"VE".to_vec(), Object::Array(vec![Object::name("And"), Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))])),
            Some("and_single_not") => membership.set(
                b"VE".to_vec(),
                Object::Array(vec![Object::name("Not"), Object::Array(vec![Object::name("And"), Object::Ref(ObjRef::new(7, 0))])]),
            ),
            Some("and_single_off") => membership.set(b"VE".to_vec(), Object::Array(vec![Object::name("And"), Object::Ref(ObjRef::new(8, 0))])),
            Some("or_single_on") => membership.set(b"VE".to_vec(), Object::Array(vec![Object::name("Or"), Object::Ref(ObjRef::new(7, 0))])),
            Some("or_single_off") => membership.set(b"VE".to_vec(), Object::Array(vec![Object::name("Or"), Object::Ref(ObjRef::new(8, 0))])),
            Some("not_off") => membership.set(b"VE".to_vec(), Object::Array(vec![Object::name("Not"), Object::Ref(ObjRef::new(8, 0))])),
            Some("malformed_not") => membership
                .set(b"VE".to_vec(), Object::Array(vec![Object::name("Not"), Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))])),
            Some("malformed_without_ocgs") => {
                membership.set(b"VE".to_vec(), Object::Array(vec![Object::name("UnknownOperator"), Object::Ref(ObjRef::new(8, 0))]))
            }
            _ => {}
        }
        doc.set(ObjRef::new(9, 0), Object::Dict(membership));
        let root = doc.root().unwrap();
        doc.update_dict(root, |d| {
            d.set(
                b"OCProperties".to_vec(),
                Object::Dict(Dict::from_iter([
                    (b"OCGs".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))])),
                    (
                        b"D".to_vec(),
                        Object::Dict(Dict::from_iter([
                            (b"Order".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0)), Object::Ref(ObjRef::new(8, 0))])),
                            (b"OFF".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(8, 0))])),
                        ])),
                    ),
                ])),
            );
        })
        .unwrap();

        remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        let has_secret = pdfcraft_edit::text_lines(&saved, 0).unwrap().iter().any(|line| line.text.contains("SECRET"));
        assert_eq!(has_secret, !should_remove, "policy {policy:?}, malformed={malformed_policy}, VE={visibility:?}");
    }
}

#[test]
fn redaction_clears_pixels_intersected_without_covering_their_centres() {
    for (content, rect) in [
        (&b"q 200 0 0 100 0 0 cm /Im Do Q"[..], [10.0, 10.0, 20.0, 90.0]),
        (&b"q 0 100 -200 0 200 0 cm /Im Do Q"[..], [10.0, 5.0, 190.0, 10.0]),
        (&b"q 200 40 30 100 0 0 cm /Im Do Q"[..], [10.0, 10.0, 20.0, 15.0]),
    ] {
        let mut doc = one_page(
            content,
            "/XObject << /Im 7 0 R >>",
            vec![stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234])],
        );
        mark(&mut doc, 0, &[rect], "");
        let report = apply(&mut doc, None).unwrap();
        assert_eq!(report.images_cleared, 1);
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        assert_eq!(named_image(&saved, 0, b"Im"), None);
        assert_eq!(named_image(&saved, 0, b"PCRedacted1"), Some(vec![0, 234]));
    }
}

#[test]
fn intersected_image_samples_keep_mask_polarity_and_packed_neighbours() {
    for (extra, original, cleared) in [
        ("/ImageMask true /BitsPerComponent 1", 0b0000_0000, 0b1000_0000),
        ("/ImageMask true /BitsPerComponent 1 /Decode [1 0]", 0b1100_0000, 0b0100_0000),
        ("/ColorSpace /DeviceGray /BitsPerComponent 4", 0xAB, 0x0B),
    ] {
        let mut doc = one_page(
            b"q 200 0 0 100 0 0 cm /Im Do Q",
            "/XObject << /Im 7 0 R >>",
            vec![stream(&format!("/Type /XObject /Subtype /Image /Width 2 /Height 1 {extra}"), &[original])],
        );
        mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
        assert_eq!(apply(&mut doc, None).unwrap().images_cleared, 1);
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        assert_eq!(named_image(&saved, 0, b"PCRedacted1"), Some(vec![cleared]), "{extra}");
    }
}

fn inherited_image_document(second_page: bool) -> Document {
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [3 0 R {}] /Count {} /MediaBox [0 0 300 300] /Resources << /XObject << /Im 7 0 R >> >> >>",
            if second_page { "8 0 R" } else { "" },
            if second_page { 2 } else { 1 }
        )
        .into_bytes(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
        stream("", b"q 200 0 0 100 0 0 cm /Im Do Q"),
        FONT.replace("95 0 R", "6 0 R").into_bytes(),
        widths(),
        stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234]),
    ];
    if second_page {
        objects.push(b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec());
    }
    pdf(objects)
}

#[test]
fn intersected_image_cleanup_handles_inherited_resources_and_shared_pages() {
    for second_page in [false, true] {
        let mut doc = inherited_image_document(second_page);
        mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
        assert_eq!(apply(&mut doc, Some(&[0])).unwrap().images_cleared, 1);
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        assert_eq!(named_image(&saved, 0, b"Im"), None);
        assert_eq!(named_image(&saved, 0, b"PCRedacted1"), Some(vec![0, 234]));
        if second_page {
            assert_eq!(named_image(&saved, 1, b"Im"), Some(vec![123, 234]));
        } else {
            assert_no_stream_payload(&saved, &[123, 234]);
        }
    }
}

#[test]
fn an_image_drawn_again_outside_the_mark_keeps_its_original() {
    let mut doc = one_page(
        b"q 200 0 0 100 0 0 cm /Im Do Q q 50 0 0 50 220 220 cm /Im Do Q",
        "/XObject << /Im 7 0 R >>",
        vec![stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234])],
    );
    mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
    assert_eq!(apply(&mut doc, None).unwrap().images_cleared, 1);
    let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
    assert_eq!(named_image(&saved, 0, b"Im"), Some(vec![123, 234]));
    assert_eq!(named_image(&saved, 0, b"PCRedacted1"), Some(vec![0, 234]));
}

#[test]
fn an_unused_form_alias_cannot_retain_the_original_image() {
    for subtype in ["/Form", "9 0 R"] {
        let mut doc = one_page(
            b"q 200 0 0 100 0 0 cm /Im Do Q",
            "/XObject << /Im 7 0 R /Unused 8 0 R >>",
            vec![
                stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234]),
                stream(&format!("/Type /XObject /Subtype {subtype} /BBox [0 0 1 1] /Resources << /XObject << /Alias 7 0 R >> >>"), b""),
                b"/Form".to_vec(),
            ],
        );
        mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
        assert_eq!(apply(&mut doc, None).unwrap().images_cleared, 1);
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        assert_no_stream_payload(&saved, &[123, 234]);
    }
}

#[test]
fn an_indirect_subtype_form_keeps_an_inherited_image_drawn_outside_the_mark() {
    let mut doc = one_page(
        b"q 200 0 0 100 0 0 cm /Im Do Q q 50 0 0 50 220 220 cm /Fm Do Q",
        "/XObject << /Im 7 0 R /Fm 8 0 R >>",
        vec![
            stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234]),
            stream("/Type /XObject /Subtype 9 0 R /BBox [0 0 1 1]", b"/Im Do"),
            b"/Form".to_vec(),
        ],
    );
    mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
    assert_eq!(apply(&mut doc, None).unwrap().images_cleared, 1);
    let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
    assert_eq!(named_image(&saved, 0, b"Im"), Some(vec![123, 234]));
    assert_eq!(named_image(&saved, 0, b"PCRedacted1"), Some(vec![0, 234]));
}

#[test]
fn unused_non_xobject_resource_aliases_refuse_incomplete_cleanup() {
    for (extra, owner) in [
        (
            "/Pattern << /P 8 0 R >>",
            stream("/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 1 1] /XStep 1 /YStep 1 /Resources 9 0 R", b""),
        ),
        ("/Font << /T3 8 0 R >>", b"<< /Type /Font /Subtype /Type3 /Resources 9 0 R >>".to_vec()),
        ("/ExtGState << /G 8 0 R >>", b"<< /SMask << /S /Alpha /G 10 0 R >> >>".to_vec()),
    ] {
        let mut doc = one_page(
            b"q 200 0 0 100 0 0 cm /Im Do Q",
            &format!("{extra} /XObject << /Im 7 0 R >>"),
            vec![
                stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234]),
                owner,
                b"<< /XObject << /Alias 7 0 R >> >>".to_vec(),
                stream("/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources 9 0 R", b""),
            ],
        );
        mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
        assert_eq!(apply(&mut doc, None), Err(RedactError::Residue(1)), "{extra}");
    }
}

#[test]
fn surviving_xobject_metadata_cannot_retain_a_replaced_original() {
    for subtype in ["/Image", "/Form"] {
        let mut doc = one_page(
            b"q 200 0 0 100 0 0 cm /Im Do Q",
            "/XObject << /Im 7 0 R /Unused 8 0 R >>",
            vec![
                stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234]),
                stream(&format!("/Type /XObject /Subtype {subtype} /BBox [0 0 1 1] /PieceInfo << /Private [9 0 R] >>"), b""),
                b"<< /Alias 7 0 R >>".to_vec(),
            ],
        );
        mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
        assert_eq!(apply(&mut doc, None), Err(RedactError::Residue(1)), "{subtype}");
    }
}

#[test]
fn form_owned_aliases_keep_live_originals_and_drop_unused_ones() {
    for draw_original in [false, true] {
        let mut doc = one_page(
            b"q 200 0 0 100 0 0 cm /Im Do Q q 50 0 0 50 220 220 cm /Fm Do Q",
            "/XObject << /Im 7 0 R /Fm 8 0 R >>",
            vec![
                stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234]),
                stream(
                    "/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Resources << /XObject << /Alias 7 0 R >> >>",
                    if draw_original { b"/Alias Do" } else { b"" },
                ),
            ],
        );
        mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
        assert_eq!(apply(&mut doc, None).unwrap().images_cleared, 1);
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        let page = pdfcraft_model::pages(&saved).remove(0);
        let res = saved.resolve(page.dict.get(b"Resources").unwrap());
        let xo = saved.resolve(res.as_dict().unwrap().get(b"XObject").unwrap());
        let form = saved.resolve(xo.as_dict().unwrap().get(b"Fm").unwrap());
        let form_res = saved.resolve(form.as_dict().unwrap().get(b"Resources").unwrap());
        let inner = saved.resolve(form_res.as_dict().unwrap().get(b"XObject").unwrap());
        assert_eq!(inner.as_dict().unwrap().contains(b"Alias"), draw_original);
        if !draw_original {
            assert_no_stream_payload(&saved, &[123, 234]);
        }
    }
}

#[test]
fn uncertain_inherited_resource_users_fail_closed_during_cleanup() {
    for (extra, content) in [
        ("/Font << /T3 << /Type /Font /Subtype /Type3 >> >>", &b"BT /T3 10 Tf (A) Tj ET"[..]),
        ("/Pattern << /P << /PatternType 1 >> >>", &b"/Pattern cs /P scn 0 0 1 1 re f"[..]),
        ("/ExtGState << /G << /SMask << /S /Alpha >> >> >>", &b"/G gs"[..]),
    ] {
        let mut doc = one_page(content, &format!("{extra} /XObject << /Im 7 0 R >>"), vec![stream("/Type /XObject /Subtype /Image", b"PRIVATE")]);
        let page = pdfcraft_model::pages(&doc).remove(0);
        let res = doc.resolve(page.dict.get(b"Resources").unwrap()).as_dict().cloned().unwrap();
        let mut private = res.clone();
        assert_eq!(crate::interp::prune_xobjects(&doc, &mut private, &[ObjRef::new(7, 0)], &[content.to_vec()]), Err(RedactError::Residue(1)));
        assert_eq!(crate::interp::unlink_doomed(&mut doc, &[ObjRef::new(7, 0)]), Err(RedactError::Residue(1)));
    }
}

#[test]
fn cyclic_and_excessive_visibility_expressions_are_bounded_and_removed() {
    use crate::sanitize::{Hidden, remove_hidden};
    for expression in [Object::Ref(ObjRef::new(10, 0)), Object::Array(vec![Object::name("And"); 4098])] {
        let mut doc = one_page(
            b"/OC /M BDC BT /F1 10 Tf 10 100 Td (SECRET) Tj ET EMC",
            "/Properties << /M 9 0 R >>",
            vec![b"<< /Type /OCG /Name (On) >>".to_vec()],
        );
        doc.set(ObjRef::new(9, 0), Object::Dict(Dict::from_iter([(b"Type".to_vec(), Object::name("OCMD")), (b"VE".to_vec(), expression)])));
        doc.set(ObjRef::new(10, 0), Object::Array(vec![Object::name("Not"), Object::Ref(ObjRef::new(10, 0))]));
        remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        assert_no_stream_payload(&saved, b"SECRET");
    }
}

#[test]
fn hidden_ocmd_resolves_indirect_types_and_visibility_operators() {
    use crate::sanitize::{Hidden, remove_hidden};
    for membership in ["<< /Type 12 0 R /OCGs [7 0 R] /P /AllOff >>", "<< /Type /OCMD /VE [13 0 R 7 0 R] >>"] {
        let mut doc = one_page(
            b"/OC /M BDC BT /F1 10 Tf 10 100 Td (SECRET) Tj ET EMC",
            "/Properties << /M 9 0 R >>",
            vec![b"<< /Type 14 0 R /Name (On) >>".to_vec(), b"null".to_vec(), membership.as_bytes().to_vec()],
        );
        doc.set(ObjRef::new(12, 0), Object::name("OCMD"));
        doc.set(ObjRef::new(13, 0), Object::name("Not"));
        doc.set(ObjRef::new(14, 0), Object::name("OCG"));
        remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        assert_no_stream_payload(&saved, b"SECRET");
    }
}

#[test]
fn hidden_ocmd_removal_preserves_later_graphics_state_and_text_advance() {
    use crate::sanitize::{Hidden, remove_hidden};
    for content in [
        &b"/OC /M BDC 1 0 0 1 100 0 cm 1 0 0 rg EMC BT /F1 10 Tf 10 100 Td (VISIBLE) Tj ET"[..],
        &b"BT /F1 10 Tf 10 100 Td /OC /M BDC (SECRET) Tj EMC (VISIBLE) Tj ET"[..],
    ] {
        let mut doc = one_page(
            content,
            "/Properties << /M 9 0 R >>",
            vec![b"<< /Type /OCG /Name (On) >>".to_vec(), b"null".to_vec(), b"<< /Type /OCMD /OCGs [7 0 R] /P /AllOff >>".to_vec()],
        );
        let before = pdfcraft_edit::text_lines(&doc, 0).unwrap();
        let visible_before = before.iter().find(|line| line.text.contains("VISIBLE")).unwrap();
        let expected_x = if content.starts_with(b"BT") { 40.0 } else { visible_before.rect[0] };
        remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        let lines = pdfcraft_edit::text_lines(&saved, 0).unwrap();
        let visible = lines.iter().find(|line| line.text.contains("VISIBLE")).unwrap();
        assert!((visible.rect[0] - expected_x).abs() < 0.001, "{lines:?}");
        assert_eq!(visible.color, visible_before.color);
        assert_no_stream_payload(&saved, b"SECRET");
    }
}

#[test]
fn malformed_xobject_resource_shapes_refuse_incomplete_cleanup() {
    for (alias, container) in [
        ("[7 0 R]", b"null".to_vec()),
        ("8 0 R", b"[7 0 R]".to_vec()),
        ("8 0 R", b"<< /Subtype /Form /Resources << /XObject << /Alias 7 0 R >> >> >>".to_vec()),
        ("8 0 R", b"7 0 R".to_vec()),
        ("8 0 R", stream("/Subtype /Form /BBox [0 0 1 1] /Resources [7 0 R]", b"")),
        ("8 0 R", stream("/Subtype /Form /BBox [0 0 1 1] /Resources << /XObject [7 0 R] >>", b"")),
    ] {
        let mut doc = one_page(
            b"q 200 0 0 100 0 0 cm /Im Do Q",
            &format!("/XObject << /Im 7 0 R /Unused {alias} >>"),
            vec![stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234]), container],
        );
        mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
        assert_eq!(apply(&mut doc, None), Err(RedactError::Residue(1)), "{alias}");
    }
}

#[test]
fn surviving_nested_form_metadata_cannot_retain_a_replaced_original() {
    for private_alias in [false, true] {
        let mut doc = one_page(
            b"/Outer Do",
            "/XObject << /Outer 9 0 R >>",
            vec![
                stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234]),
                stream(
                    &format!(
                        "/Subtype /Form /BBox [0 0 1 1] /Resources << /XObject << /Alias 7 0 R >> >> {}",
                        if private_alias { "/PieceInfo << /Private 7 0 R >>" } else { "" }
                    ),
                    b"",
                ),
                stream(
                    "/Subtype /Form /BBox [0 0 300 300] /Resources << /XObject << /Im 7 0 R /Fm 8 0 R >> >>",
                    b"q 200 0 0 100 0 0 cm /Im Do Q q 50 0 0 50 220 220 cm /Fm Do Q",
                ),
            ],
        );
        mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
        let result = apply(&mut doc, None);
        if private_alias {
            assert_eq!(result, Err(RedactError::Residue(1)));
        } else {
            result.unwrap();
            assert_no_stream_payload(&reopen(&doc), &[123, 234]);
        }
    }
}

#[test]
fn malformed_page_resources_refuse_retaining_a_replaced_original() {
    let mut doc = one_page(
        b"q 200 0 0 100 0 0 cm /Im Do Q",
        "/XObject << /Im 7 0 R >>",
        vec![stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234])],
    );
    let page = doc.add(Object::Dict(Dict::from_iter([
        (b"Type".to_vec(), Object::name("Page")),
        (b"Parent".to_vec(), Object::Ref(ObjRef::new(2, 0))),
        (b"MediaBox".to_vec(), Object::Array(vec![0.into(), 0.into(), 300.into(), 300.into()])),
        (b"Resources".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(7, 0))])),
    ])));
    doc.update_dict(ObjRef::new(2, 0), |d| {
        d.set(b"Kids".to_vec(), Object::Array(vec![Object::Ref(ObjRef::new(3, 0)), Object::Ref(page)]));
        d.set(b"Count".to_vec(), 2);
    })
    .unwrap();
    mark(&mut doc, 0, &[[10.0, 10.0, 20.0, 90.0]], "");
    assert_eq!(apply(&mut doc, None), Err(RedactError::Residue(1)));
}

#[test]
fn hidden_text_advances_use_extgstate_fonts_and_restore_them_with_q() {
    use crate::sanitize::{Hidden, remove_hidden};
    for (state, expected_x) in [("/G gs", 70.0), ("q /G gs Q", 40.0)] {
        let data = format!("BT /F1 10 Tf 10 100 Td {state} /OC /M BDC (SECRET) Tj EMC (VISIBLE) Tj ET");
        let mut doc = one_page(
            data.as_bytes(),
            "/Font << /F1 5 0 R /F2 7 0 R >> /ExtGState << /G << /Font [7 0 R 10] >> >> /Properties << /M 11 0 R >>",
            vec![
                FONT.replace("95 0 R", "8 0 R").into_bytes(),
                format!("[{}]", vec!["1000"; 95].join(" ")).into_bytes(),
                b"<< /Type /OCG /Name (On) >>".to_vec(),
                b"null".to_vec(),
                b"<< /Type /OCMD /OCGs [9 0 R] /P /AllOff >>".to_vec(),
            ],
        );
        remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
        let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
        let lines = pdfcraft_edit::text_lines(&saved, 0).unwrap();
        let visible = lines.iter().find(|line| line.text.contains("VISIBLE")).unwrap();
        assert!((visible.rect[0] - expected_x).abs() < 0.001, "{state}: {lines:?}");
        assert_no_stream_payload(&saved, b"SECRET");
    }
}

#[test]
fn hidden_composite_text_refuses_unestablished_font_advances() {
    use crate::sanitize::{Hidden, remove_hidden};
    for (subtype, encoding) in [("12 0 R", "Identity-H"), ("/Type0", "UniJIS-UCS2-H"), ("/Type0", "V"), ("/Type0", "Identity-H")] {
        for state in ["/F2 10 Tf", "/G gs"] {
            let data = format!("BT /F1 10 Tf 10 100 Td {state} /OC /M BDC <0001> Tj EMC /F1 10 Tf (VISIBLE) Tj ET");
            let mut doc = one_page(
                data.as_bytes(),
                "/Font << /F1 5 0 R /F2 7 0 R >> /ExtGState << /G << /Font [7 0 R 10] >> >> /Properties << /M 11 0 R >>",
                vec![
                    format!("<< /Type /Font /Subtype {subtype} /BaseFont /X /Encoding /{encoding} /DescendantFonts [<< /Subtype /CIDFontType2 /DW 2000 >>] >>")
                        .into_bytes(),
                    b"null".to_vec(),
                    b"<< /Type /OCG /Name (On) >>".to_vec(),
                    b"null".to_vec(),
                    b"<< /Type /OCMD /OCGs [9 0 R] /P /AllOff >>".to_vec(),
                    b"/Type0".to_vec(),
                ],
            );
            let result = remove_hidden(&mut doc, &[Hidden::HiddenLayers]);
            if subtype == "/Type0" && encoding == "Identity-H" {
                result.unwrap();
                let saved = reopen(&doc);
                let lines = pdfcraft_edit::text_lines(&saved, 0).unwrap();
                let visible = lines.iter().find(|line| line.text.contains("VISIBLE")).unwrap();
                assert!((visible.rect[0] - 30.0).abs() < 0.001, "{state}: {lines:?}");
                assert_no_stream_payload(&saved, &[0, 1]);
            } else {
                assert_eq!(result, Err(RedactError::Residue(1)), "{subtype} {encoding} {state}");
            }
        }
    }
}

#[test]
fn hidden_ocmd_painting_is_removed_while_clipping_state_remains() {
    use crate::sanitize::{Hidden, remove_hidden};
    let mut doc = one_page(
        b"/OC /M BDC 1 2 3 4 re f 10 20 30 40 re W f q 10 0 0 10 0 0 cm /Im Do Q EMC 0 0 100 100 re f",
        "/Properties << /M 9 0 R >> /XObject << /Im 10 0 R >>",
        vec![
            b"<< /Type /OCG /Name (On) >>".to_vec(),
            b"null".to_vec(),
            b"<< /Type /OCMD /OCGs [7 0 R] /P /AllOff >>".to_vec(),
            stream("/Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[123, 234]),
        ],
    );
    remove_hidden(&mut doc, &[Hidden::HiddenLayers]).unwrap();
    let saved = Document::open(Arc::new(write_full(&doc, &SaveOptions::default()).unwrap())).unwrap();
    let ops = content(&saved, 0);
    assert!(!ops.contains("1 2 3 4 re"));
    assert!(ops.contains("10 20 30 40 re\nW\nn"), "{ops}");
    assert!(!ops.contains("/Im Do"));
    assert_no_stream_payload(&saved, &[123, 234]);
}

#[test]
fn hidden_clipping_text_refuses_a_state_changing_rewrite() {
    use crate::sanitize::{Hidden, remove_hidden};
    let mut doc = one_page(
        b"/OC /M BDC BT /F1 10 Tf 7 Tr 10 100 Td (SECRET) Tj ET EMC",
        "/Properties << /M 9 0 R >>",
        vec![b"<< /Type /OCG /Name (On) >>".to_vec(), b"null".to_vec(), b"<< /Type /OCMD /OCGs [7 0 R] /P /AllOff >>".to_vec()],
    );
    assert_eq!(remove_hidden(&mut doc, &[Hidden::HiddenLayers]), Err(RedactError::Residue(1)));
}

#[test]
fn hidden_zero_size_text_with_spacing_refuses_a_state_changing_rewrite() {
    use crate::sanitize::{Hidden, remove_hidden};
    let mut doc = one_page(
        b"BT /F1 0 Tf 5 Tc /OC /M BDC (SECRET) Tj EMC /F1 10 Tf (VISIBLE) Tj ET",
        "/Properties << /M 9 0 R >>",
        vec![b"<< /Type /OCG /Name (On) >>".to_vec(), b"null".to_vec(), b"<< /Type /OCMD /OCGs [7 0 R] /P /AllOff >>".to_vec()],
    );
    assert_eq!(remove_hidden(&mut doc, &[Hidden::HiddenLayers]), Err(RedactError::Residue(1)));
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut v = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    v.extend_from_slice(data);
    v.extend_from_slice(b"\nendstream");
    v
}

fn pdf(objs: Vec<Vec<u8>>) -> Document {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    Document::open(Arc::new(out)).unwrap()
}

/// A font where every glyph is 500 units wide (so a 10 pt glyph advances 5 pt).
const FONT: &str = "<< /Type /Font /Subtype /TrueType /BaseFont /Arial /FirstChar 32 /LastChar 126 /Widths 95 0 R /FontDescriptor << /Ascent 800 /Descent -200 >> >>";

fn widths() -> Vec<u8> {
    format!("[{}]", vec!["500"; 95].join(" ")).into_bytes()
}

/// One 300×300 page with `content`, font /F1, and extra resources/objects.
fn one_page(content: &[u8], extra_res: &str, extra: Vec<Vec<u8>>) -> Document {
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> {extra_res} >> >>")
            .into_bytes(),
        stream("", content),
        FONT.replace("95 0 R", "6 0 R").into_bytes(),
        widths(),
    ];
    objs.extend(extra);
    pdf(objs)
}

fn mark(doc: &mut Document, page: usize, rects: &[[f64; 4]], overlay: &str) {
    let shape = Shape::Redact { quads: rects.iter().map(|r| rect_quad(*r)).collect(), overlay: overlay.into(), look: Default::default() };
    let style = Style::default_for(&shape);
    add_annotation(doc, &NewAnnotation { page, shape, style, contents: String::new(), author: "Tester".into() }, &Meta::default()).unwrap();
}

/// The decoded content streams of a page, joined.
fn content(doc: &Document, page: usize) -> String {
    let p = &pdfcraft_model::pages(doc)[page];
    let (_, data) = page_streams(doc, &p.dict, page).unwrap();
    data.iter().map(|d| String::from_utf8_lossy(d).into_owned()).collect::<Vec<_>>().join("\n")
}

/// Glyphs (and inline images) of page `page` under `rects` (the verifier's count).
fn under(doc: &mut Document, page: usize, rects: &[[f64; 4]]) -> usize {
    let p = pdfcraft_model::pages(doc).swap_remove(page);
    let (_, data) = page_streams(doc, &p.dict, page).unwrap();
    let res = p.dict.get(b"Resources").and_then(|r| doc.resolve(r).as_dict().cloned()).unwrap_or_default();
    let mut rep = Report::default();
    let mut scope = Scope::new(rects, Mode::Verify, &mut rep);
    process(doc, &mut scope, &data, &res, pdfcraft_content::Matrix::IDENTITY).residue
}

fn reopen(doc: &Document) -> Document {
    Document::open(Arc::new(write_incremental(doc, &SaveOptions::default()).unwrap())).unwrap()
}

#[test]
fn glyphs_under_a_mark_go_and_the_rest_stays_put() {
    // "AB1234CD" from x = 10 at 10 pt: each glyph 5 pt wide; 1234 spans x 20–40.
    let mut doc = one_page(b"BT /F1 10 Tf 10 100 Td (AB1234CD) Tj ET", "", vec![]);
    assert_eq!(under(&mut doc, 0, &[[40.0, 95.0, 50.0, 110.0]]), 2, "C and D before");
    mark(&mut doc, 0, &[[20.0, 95.0, 40.0, 110.0]], "");
    let r = apply(&mut doc, None).unwrap();
    assert_eq!((r.marks, r.glyphs), (1, 4));
    let c = content(&doc, 0);
    assert!(c.contains("[(AB) -2000 (CD)] TJ"), "{c}");
    assert!(!c.contains("1234"));
    assert_eq!(under(&mut doc, 0, &[[40.0, 95.0, 50.0, 110.0]]), 2, "C and D are where they were");
    assert_eq!(under(&mut doc, 0, &[[20.0, 95.0, 40.0, 110.0]]), 0);
    // The mark became a black box drawn into the page; the annotation is gone.
    assert!(marks(&doc).is_empty());
    assert!(c.contains("0 0 0 rg") && c.contains("20 95 20 15 re f"), "{c}");
    let doc = reopen(&doc);
    assert!(!content(&doc, 0).contains("1234"));
}

#[test]
fn added_text_parameters_do_not_keep_redacted_text() {
    // An Edit ▸ Add content item: its stream dictionary keeps the source text under /PCAdded.
    let mut doc = pdf(vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents [4 0 R] /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
        stream("/PCMark /Added /PCAdded << /Kind /Text /Text (AB1234CD) >>", b"BT /F1 10 Tf 10 100 Td (AB1234CD) Tj ET"),
        FONT.replace("95 0 R", "6 0 R").into_bytes(),
        widths(),
    ]);
    mark(&mut doc, 0, &[[20.0, 95.0, 40.0, 110.0]], "");
    apply(&mut doc, None).unwrap();
    assert!(!content(&doc, 0).contains("1234"));
    let bytes = write_full(&doc, &SaveOptions::default()).unwrap();
    assert!(!bytes.windows(4).any(|w| w == b"1234"), "redacted text left in the saved file");
}

#[test]
fn operators_split_across_content_streams_are_redacted_whole() {
    // One content stream in three pieces, split between tokens: a marked-content dictionary
    // ends in the second piece, and the TJ array that ends it shows its glyphs with the
    // operator in the third.
    let mut doc = pdf(vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents [4 0 R 7 0 R 8 0 R] /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
        stream("", b"/P << /MCID 0"),
        FONT.replace("95 0 R", "6 0 R").into_bytes(),
        widths(),
        stream("", b">> BDC BT /F1 10 Tf 10 100 Td [(AB1234CD)]"),
        stream("", b"TJ ET EMC BT /F1 10 Tf 10 200 Td (KEEP) Tj ET"),
    ]);
    assert_eq!(under(&mut doc, 0, &[[20.0, 95.0, 40.0, 110.0]]), 4, "the verifier sees the split TJ");
    mark(&mut doc, 0, &[[20.0, 95.0, 40.0, 110.0]], "");
    let r = apply(&mut doc, None).unwrap();
    assert_eq!(r.glyphs, 4, "{r:?}");
    let c = content(&doc, 0);
    assert!(!c.contains("1234"), "redacted glyphs left in the page: {c}");
    assert!(c.contains("KEEP"), "{c}");
    assert_eq!(under(&mut doc, 0, &[[40.0, 95.0, 50.0, 110.0]]), 2, "C and D are where they were");
    // Every operator still has its operands: none was cut off from them.
    for op in pdfcraft_content::parse(c.as_bytes()).ops {
        let want = match op.op.as_slice() {
            b"TJ" | b"Tj" => 1,
            b"BDC" | b"Tf" | b"Td" => 2,
            _ => continue,
        };
        assert_eq!(op.operands.len(), want, "{} lost its operands in {c}", String::from_utf8_lossy(&op.op));
    }
    let doc = reopen(&doc);
    assert!(!content(&doc, 0).contains("1234"));
}

#[test]
fn kerning_spacing_scaling_and_line_operators_are_honoured() {
    // TJ kerning, character and word spacing, 50% horizontal scaling, ' and ".
    let src = b"BT /F1 10 Tf 2 Tc 4 Tw 50 Tz 12 TL 0 200 Td [(AB) -1000 (C D)] TJ (EF) ' 1 0 (GH) \" ET";
    let mut doc = one_page(src, "", vec![]);
    // Line 1 (y 200): A at 0, B at 3.5 (advance (5+2)×0.5), kern +5 → C at 12, space at 15.5,
    // D at 21 (space advance (5+2+4)×0.5 = 5.5). Remove C only.
    mark(&mut doc, 0, &[[12.2, 195.0, 14.8, 210.0]], "");
    // Line 2 (y 188) "EF" and line 3 (y 176) "GH" (Tw 1, Tc 0): remove F and G.
    mark(&mut doc, 0, &[[3.8, 183.0, 6.0, 198.0 - 10.0], [0.2, 171.0, 2.3, 180.0]], "");
    let r = apply(&mut doc, None).unwrap();
    assert_eq!(r.glyphs, 3, "{r:?}");
    let c = content(&doc, 0);
    assert!(c.contains("[(AB) -1700 ( D)] TJ"), "{c}");
    assert!(c.contains("T*\n[(E) -700] TJ"), "{c}");
    assert!(c.contains("1 Tw\n0 Tc\nT*\n[-500 (H)] TJ"), "{c}");
    // Everything else stays readable where it was.
    assert_eq!(under(&mut doc, 0, &[[20.0, 195.0, 30.0, 210.0]]), 1, "D");
    assert_eq!(under(&mut doc, 0, &[[2.6, 171.0, 6.0, 180.0]]), 1, "H");
}

#[test]
fn rotated_text_and_composite_fonts() {
    // A Type0 Identity-H font (two-byte codes) with /W widths, and a 90° text matrix.
    let t0 = b"<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H /DescendantFonts [<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X /DW 1000 /W [1 [500 500] 3 4 250] >>] >>".to_vec();
    let src = b"BT /F2 10 Tf 0 1 -1 0 100 50 Tm <0001000200030004> Tj ET";
    let mut doc = one_page(src, "", vec![]);
    let font_ref = doc.add(Object::Dict(Dict::new()));
    let parsed = {
        let mut lx = pdfcraft_cos::Lexer::new(&t0, 0);
        lx.object().unwrap()
    };
    doc.set(font_ref, parsed);
    let page = pdfcraft_model::pages(&doc)[0].obj;
    doc.update_dict(page, |d| {
        let mut res = d.get(b"Resources").and_then(Object::as_dict).cloned().unwrap();
        let mut fonts = res.get(b"Font").and_then(Object::as_dict).cloned().unwrap();
        fonts.set(b"F2".to_vec(), Object::Ref(font_ref));
        res.set(b"Font".to_vec(), Object::Dict(fonts));
        d.set(b"Resources".to_vec(), Object::Dict(res));
    })
    .unwrap();
    // Glyphs run upwards from y 50: CID1 50–55, CID2 55–60, CID3 60–62.5, CID4 62.5–65.
    mark(&mut doc, 0, &[[85.0, 55.5, 105.0, 61.0]], "");
    let r = apply(&mut doc, None).unwrap();
    assert_eq!(r.glyphs, 2, "CIDs 2 and 3");
    let c = content(&doc, 0);
    assert!(c.contains("[<0001> -750 <0004>] TJ") || c.contains("[(\\000\\001) -750 (\\000\\004)] TJ"), "{c}");
}

#[test]
fn images_are_removed_or_have_their_pixels_cleared() {
    // Image 1 is fully covered; image 2 (4×1 gray, all white) is half covered.
    let img = stream("/Type /XObject /Subtype /Image /Width 4 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8", &[255, 255, 255, 255]);
    let src = b"q 10 0 0 10 10 10 cm /Im1 Do Q q 40 0 0 10 100 100 cm /Im2 Do Q";
    let mut doc = one_page(src, "/XObject << /Im1 7 0 R /Im2 7 0 R >>", vec![img]);
    mark(&mut doc, 0, &[[5.0, 5.0, 25.0, 25.0], [95.0, 95.0, 120.0, 115.0]], "");
    let r = apply(&mut doc, None).unwrap();
    assert_eq!((r.images_removed, r.images_cleared), (1, 1), "{r:?}");
    let c = content(&doc, 0);
    assert!(!c.contains("/Im1 Do") && !c.contains("/Im2 Do"), "{c}");
    let p = pdfcraft_model::pages(&doc).swap_remove(0);
    let res = doc.resolve(p.dict.get(b"Resources").unwrap()).as_dict().cloned().unwrap();
    let xo = doc.resolve(res.get(b"XObject").unwrap()).as_dict().cloned().unwrap();
    let new = xo.iter().find(|(k, _)| k.starts_with(b"PCRedacted")).map(|(_, v)| v.clone()).expect("a cleared copy");
    let Object::Stream(s) = &*doc.resolve(&new) else { panic!() };
    assert_eq!(s.decoded().unwrap(), [0, 0, 255, 255], "pixels 1–2 (x 100–120) cleared");
    let Object::Stream(orig) = &*doc.get(ObjRef::new(7, 0)) else { panic!() };
    assert_eq!(orig.decoded().unwrap(), [255; 4], "the shared original is untouched");
}

#[test]
fn vectors_are_removed_or_clipped_and_inline_images_go() {
    let src = b"0 g 10 10 20 20 re f 0 0 300 300 re f q 10 0 0 10 50 50 cm BI /W 1 /H 1 /CS /G /BPC 8 ID \x80 EI Q 1 0 0 RG 150 150 m 160 160 l S";
    let mut doc = one_page(src, "", vec![]);
    mark(&mut doc, 0, &[[5.0, 5.0, 35.0, 35.0], [45.0, 45.0, 65.0, 65.0]], "");
    let r = apply(&mut doc, None).unwrap();
    assert_eq!((r.paths_removed, r.paths_clipped, r.images_removed), (1, 1, 1), "{r:?}");
    let c = content(&doc, 0);
    assert!(!c.contains("10 10 20 20 re"), "{c}");
    assert!(c.contains("W*\nn\n0 0 300 300 re\nf\nQ"), "the page-size rect is clipped: {c}");
    assert!(!c.contains("BI"), "{c}");
    assert!(c.contains("150 150 m"), "the line elsewhere stays");
}

#[test]
fn shared_form_xobjects_are_copied_not_changed() {
    let form =
        stream("/Type /XObject /Subtype /Form /BBox [0 0 300 300] /Resources << /Font << /F1 5 0 R >> >>", b"BT /F1 10 Tf 10 100 Td (SECRET) Tj ET");
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 8 0 R] /Count 2 /MediaBox [0 0 300 300] /Resources << /XObject << /Fm 7 0 R >> >> >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
        stream("", b"/Fm Do"),
        FONT.replace("95 0 R", "6 0 R").into_bytes(),
        widths(),
        form,
    ];
    objs.push(b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec());
    let mut doc = pdf(objs);
    mark(&mut doc, 0, &[[0.0, 90.0, 300.0, 120.0]], "");
    let r = apply(&mut doc, None).unwrap();
    assert_eq!((r.glyphs, r.forms_rewritten), (6, 1), "{r:?}");
    assert!(content(&doc, 0).contains("/PCRedacted1 Do"));
    assert_eq!(content(&doc, 1), "/Fm Do", "page 2 shares the stream and the form: untouched");
    assert_eq!(under(&mut doc, 1, &[[0.0, 90.0, 300.0, 120.0]]), 6);
    assert_eq!(under(&mut doc, 0, &[[0.0, 90.0, 300.0, 120.0]]), 0);
}

#[test]
fn comments_links_and_fields_under_a_mark_go() {
    let mut doc = one_page(b"", "", vec![]);
    // A comment under the mark, a link elsewhere, and a text field under the mark.
    let square = Shape::Rectangle { rect: [20.0, 20.0, 40.0, 40.0] };
    add_annotation(
        &mut doc,
        &NewAnnotation { page: 0, style: Style::default_for(&square), shape: square, contents: "x".into(), author: "a".into() },
        &Meta::default(),
    )
    .unwrap();
    let link = Shape::Rectangle { rect: [200.0, 200.0, 220.0, 220.0] };
    add_annotation(
        &mut doc,
        &NewAnnotation { page: 0, style: Style::default_for(&link), shape: link, contents: "keep".into(), author: "a".into() },
        &Meta::default(),
    )
    .unwrap();
    pdfcraft_forms::add_field(&mut doc, 0, [10.0, 50.0, 100.0, 70.0], &pdfcraft_forms::NewField::Text { multiline: false }, Some("ssn")).unwrap();
    mark(&mut doc, 0, &[[0.0, 0.0, 120.0, 80.0]], "REDACTED");
    let r = apply(&mut doc, None).unwrap();
    assert_eq!((r.annotations, r.fields), (1, 1), "{r:?}");
    assert!(pdfcraft_forms::fields(&doc).is_empty());
    let p = pdfcraft_model::pages(&doc).swap_remove(0);
    assert_eq!(annots_of(&doc, &p.dict).len(), 1, "only the far rectangle stays");
    let c = content(&doc, 0);
    assert!(c.contains("(REDACTED) Tj"), "overlay text: {c}");
}

#[test]
fn nothing_to_apply_and_clearing_marks() {
    let mut doc = one_page(b"BT /F1 10 Tf 10 100 Td (AB) Tj ET", "", vec![]);
    assert_eq!(apply(&mut doc, None), Err(RedactError::NothingToApply));
    mark(&mut doc, 0, &[[0.0, 0.0, 50.0, 50.0]], "");
    assert_eq!(marks(&doc).len(), 1);
    assert_eq!(clear_marks(&mut doc, None), Ok(1));
    assert!(marks(&doc).is_empty());
    assert!(content(&doc, 0).contains("(AB) Tj"));
}

#[test]
fn unreadable_content_fails_closed() {
    let mut doc = one_page(b"", "", vec![]);
    let page = pdfcraft_model::pages(&doc)[0].obj;
    let bad = doc.add(Object::Stream(Stream::from_raw(
        {
            let mut d = Dict::new();
            d.set(b"Filter".to_vec(), Object::name("DCTDecode"));
            d
        },
        b"garbage".to_vec(),
    )));
    doc.update_dict(page, |d| d.set(b"Contents".to_vec(), Object::Ref(bad))).unwrap();
    mark(&mut doc, 0, &[[0.0, 0.0, 50.0, 50.0]], "");
    assert_eq!(apply(&mut doc, None), Err(RedactError::Unreadable(1)));
}

/// A document with something in every hidden-information category.
fn hidden_fixture() -> Document {
    let content = b"BT /F1 10 Tf 10 100 Td (Visible) Tj 3 Tr (Hidden) Tj 0 Tr ET BT /F1 10 Tf 500 500 Td (Offpage) Tj ET /OC /L1 BDC BT /F1 10 Tf 10 50 Td (Layer) Tj ET EMC";
    let objs: Vec<Vec<u8>> = vec![
        // 1 catalog
        b"<< /Type /Catalog /Pages 2 0 R /Metadata 7 0 R /Names << /EmbeddedFiles << /Names [(a.txt) 8 0 R] >> /JavaScript << /Names [(init) 9 0 R] >> >> /OpenAction 9 0 R /Outlines 10 0 R /PageMode /UseOutlines /PieceInfo << /App << /Private 1 >> >> /OCProperties << /OCGs [13 0 R 14 0 R] /D << /OFF [13 0 R] /Order [13 0 R 14 0 R] >> >> /AcroForm << /Fields [17 0 R] >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        // 3 page
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /Properties << /L1 13 0 R >> >> /Annots [15 0 R 16 0 R 17 0 R 18 0 R 19 0 R] /PieceInfo << /App << /Private 2 >> >> /AA << /O 9 0 R >> >>".to_vec(),
        stream("", content),
        FONT.replace("95 0 R", "6 0 R").into_bytes(),
        widths(),
        stream("/Type /Metadata /Subtype /XML", b"<x:xmpmeta/>"),
        b"<< /Type /Filespec /F (a.txt) /EF << /F 20 0 R >> >>".to_vec(),
        b"<< /S /JavaScript /JS (app.alert(1)) >>".to_vec(),
        b"<< /Type /Outlines /First 11 0 R /Last 12 0 R /Count 2 >>".to_vec(),
        b"<< /Title (One) /Parent 10 0 R /Next 12 0 R >>".to_vec(),
        b"<< /Title (Two) /Parent 10 0 R /Prev 11 0 R >>".to_vec(),
        b"<< /Type /OCG /Name (Secret layer) >>".to_vec(),
        b"<< /Type /OCG /Name (Shown layer) >>".to_vec(),
        // 15 comment + 16 its pop-up, 17 widget, 18 link, 19 attachment
        b"<< /Type /Annot /Subtype /Square /Rect [10 10 50 50] /C [1 0 0] /Popup 16 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Popup /Rect [60 10 160 60] /Parent 15 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V (Ada) /Rect [10 200 150 220] /A 9 0 R >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [10 230 50 240] /A << /S /URI /URI (https://example.org) >> >>".to_vec(),
        b"<< /Type /Annot /Subtype /FileAttachment /Rect [200 10 210 20] /FS 8 0 R >>".to_vec(),
        stream("", b"attached"),
    ];
    let mut doc = pdf(objs);
    let info = doc.add(Object::Dict({
        let mut d = Dict::new();
        d.set(b"Title".to_vec(), pdfcraft_cos::PdfString::text("Secret plan"));
        d.set(b"Author".to_vec(), pdfcraft_cos::PdfString::text("Ada"));
        d
    }));
    doc.trailer_mut().set(b"Info".to_vec(), Object::Ref(info));
    doc
}

#[test]
fn hidden_information_is_counted_and_removed() {
    use crate::sanitize::{Hidden, remove_hidden, sanitize, scan};
    let mut doc = hidden_fixture();
    let counts: std::collections::HashMap<Hidden, usize> = scan(&doc).into_iter().collect();
    assert_eq!(counts[&Hidden::Metadata], 3, "two Info entries and the XMP stream");
    assert_eq!(counts[&Hidden::Attachments], 2, "the embedded file and the attachment annotation");
    assert_eq!(counts[&Hidden::Comments], 1);
    assert_eq!(counts[&Hidden::FormFields], 1);
    assert_eq!(counts[&Hidden::HiddenText], 6 + 7, "Hidden (render mode 3) and Offpage (off the page)");
    assert_eq!(counts[&Hidden::HiddenLayers], 2, "one off layer and its block");
    assert_eq!(counts[&Hidden::Bookmarks], 2);
    assert_eq!(counts[&Hidden::LinksActionsScripts], 5, "link, open action, page actions, widget action, document script");
    assert_eq!(counts[&Hidden::PrivateData], 2);

    // Only hidden text first.
    let done = remove_hidden(&mut doc, &[Hidden::HiddenText]).unwrap();
    assert_eq!(done, [(Hidden::HiddenText, 13)]);
    let c = content(&doc, 0);
    assert!(c.contains("(Visible)") && !c.contains("Hidden") && !c.contains("Offpage") && c.contains("(Layer)"), "{c}");
    assert!(doc.full_save_required());

    // Then everything.
    let done = sanitize(&mut doc).unwrap();
    assert!(done.iter().all(|(h, _)| *h != Hidden::HiddenText));
    let c = content(&doc, 0);
    assert!(!c.contains("Layer") && c.contains("(Visible)"), "{c}");
    let doc = reopen(&doc);
    let after: Vec<(Hidden, usize)> = scan(&doc).into_iter().filter(|(_, n)| *n > 0).collect();
    // A full save writes a fresh /Info with the modification date only.
    assert!(after.iter().all(|(h, _)| *h == Hidden::Metadata), "{after:?}");
    let cat = doc.get(doc.root().unwrap()).as_dict().cloned().unwrap();
    for k in [&b"Outlines"[..], b"OpenAction", b"PieceInfo", b"AcroForm", b"Metadata"] {
        assert!(!cat.contains(k), "{}", String::from_utf8_lossy(k));
    }
    assert!(pdfcraft_forms::fields(&doc).is_empty());
    let p = pdfcraft_model::pages(&doc).swap_remove(0);
    assert!(annots_of(&doc, &p.dict).is_empty());
    let shown = doc.object_numbers().into_iter().any(|n| match &*doc.get(ObjRef::new(n, doc.generation(n))) {
        Object::Stream(s) => s.decoded().is_ok_and(|d| d.windows(5).any(|w| w == b"(Ada)")),
        _ => false,
    });
    assert!(shown, "the field's value stays visible as page content");
}

#[test]
fn overlay_text_takes_its_font_size_colour_alignment_and_repeats() {
    let mut doc = one_page(b"BT /F1 12 Tf 20 250 Td (Secret salary figures) Tj ET", "", Vec::new());
    let look = pdfcraft_annot::OverlayLook { font: pdfcraft_annot::OverlayFont::Courier, size: 8.0, color: [0.0, 0.0, 1.0], align: 0, repeat: true };
    let shape = Shape::Redact { quads: vec![rect_quad([10.0, 200.0, 290.0, 270.0])], overlay: "REDACTED".into(), look };
    let style = Style::default_for(&shape);
    add_annotation(&mut doc, &NewAnnotation { page: 0, shape, style, contents: String::new(), author: "T".into() }, &Meta::default()).unwrap();
    // Written as Acrobat writes it.
    let m = &marks(&doc)[0];
    assert_eq!(m.look, look, "the look round-trips through /DA, /Q and /Repeat");
    apply(&mut doc, None).unwrap();
    let doc = reopen(&doc);
    let c = content(&doc, 0);
    assert!(c.contains("0 0 1 rg /PCCour 8 Tf"), "{c}");
    // 70 pt high at 8 pt × 1.2 leading: several lines, each the word repeated.
    assert!(c.matches(" Tm (REDACTED REDACTED").count() >= 5, "{c}");
    assert!(c.contains("1 0 0 1 11 "), "left aligned at the area's edge: {c}");
    let p = &pdfcraft_model::pages(&doc)[0];
    let fonts = p
        .dict
        .get(b"Resources")
        .and_then(|r| doc.resolve(r).as_dict().cloned())
        .and_then(|r| r.get(b"Font").and_then(|f| doc.resolve(f).as_dict().cloned()))
        .unwrap();
    assert!(fonts.contains(b"PCCour") && !fonts.contains(b"PCTimes"));
}

#[test]
fn redaction_codes_join_in_set_order_and_reject_strangers() {
    use crate::codes::{CODE_SETS, CodeSet};
    let foia = CodeSet::from_id("foia").unwrap();
    assert_eq!(foia.overlay(&["(b)(6)", "(b)(1)(A)", "(b)(6)"]), Ok("(b)(1)(A), (b)(6)".into()));
    assert_eq!(foia.overlay(&[]), Ok(String::new()));
    assert_eq!(foia.overlay(&["(b)(6)", "(k)(1)"]), Err("(k)(1)"));
    let privacy = CodeSet::from_id("privacy-act").unwrap();
    assert_eq!(privacy.overlay(&["(k)(7)", "(d)(5)"]), Ok("(d)(5), (k)(7)".into()));
    assert_eq!(CodeSet::from_id("gdpr"), None);
    assert!(CODE_SETS.iter().all(|s| !s.codes.is_empty() && s.codes.iter().all(|c| !c.is_empty())));
}

#[test]
fn tags_lose_what_redaction_removed() {
    let content = b"/P <</MCID 0>> BDC BT /F1 10 Tf 10 200 Td (SECRET) Tj ET EMC /P <</MCID 1>> BDC BT /F1 10 Tf 10 100 Td (Public) Tj ET EMC";
    let mut doc = pdf(vec![
        b"<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 7 0 R /MarkInfo << /Marked true >> >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Contents 4 0 R /StructParents 0 /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
        stream("", content),
        FONT.replace("95 0 R", "6 0 R").into_bytes(),
        widths(),
        b"<< /Type /StructTreeRoot /K 8 0 R >>".to_vec(),
        b"<< /S /Document /P 7 0 R /K [9 0 R 10 0 R] >>".to_vec(),
        b"<< /S /P /P 8 0 R /Pg 3 0 R /ActualText (SECRET) /Alt (the secret) /K 0 >>".to_vec(),
        b"<< /S /P /P 8 0 R /Pg 3 0 R /ActualText (Public) /K 1 >>".to_vec(),
    ]);
    mark(&mut doc, 0, &[[5.0, 195.0, 60.0, 212.0]], "");
    let report = apply(&mut doc, None).unwrap();
    assert_eq!(report.tags, 1);
    let secret = doc.get(pdfcraft_cos::ObjRef::new(9, 0)).as_dict().cloned().unwrap();
    assert!(secret.get(b"ActualText").is_none() && secret.get(b"Alt").is_none());
    assert!(secret.get(b"K").is_none(), "its marked content is empty now");
    let public = doc.get(pdfcraft_cos::ObjRef::new(10, 0)).as_dict().cloned().unwrap();
    assert_eq!(public.get(b"K").and_then(Object::as_int), Some(1));
    assert!(public.get(b"ActualText").is_some(), "untouched content keeps its tags");
}
