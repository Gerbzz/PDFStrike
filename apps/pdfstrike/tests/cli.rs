use std::fs;
use std::process::Command;

use pdfcraft_annot::summaries;
use pdfcraft_cos::{Document, SaveOptions, write_full};
use pdfcraft_forms::{NewField, add_field, fields};
use pdfcraft_organize::{insert_blank_page, page_boxes, page_rotation};

fn source_pdf() -> Vec<u8> {
    let mut doc = Document::new_empty();
    for width in [400.0, 500.0, 600.0] {
        insert_blank_page(&mut doc, 0, width, 700.0).unwrap();
    }
    write_full(&doc, &SaveOptions::default()).unwrap()
}

#[test]
fn windows_command_workflow_saves_and_exports_real_pdf() {
    let root = std::env::temp_dir().join(format!("pdfstrike-cli-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.pdf");
    let project = root.join("work.pdfstrike");
    let output = root.join("review.pdf");
    fs::write(&source, source_pdf()).unwrap();
    let bin = env!("CARGO_BIN_EXE_pdfstrike");
    let run = |args: Vec<&str>| {
        let out = Command::new(bin).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let s = source.to_str().unwrap();
    let p = project.to_str().unwrap();
    let o = output.to_str().unwrap();
    run(vec!["new", s, p]);
    run(vec!["move", p, "1", "3"]);
    run(vec!["rotate", p, "1", "90"]);
    run(vec!["note", p, "1", "72", "620", "Check figure"]);
    run(vec!["highlight", p, "1", "72", "610", "180", "630"]);
    run(vec!["delete", p, "3"]);
    run(vec!["export", p, o]);
    let doc = Document::open(std::sync::Arc::new(fs::read(&output).unwrap())).unwrap();
    assert_eq!(page_boxes(&doc).unwrap().len(), 2);
    assert_eq!(page_boxes(&doc).unwrap()[0][0][2], 500.0);
    assert_eq!(page_rotation(&doc, 0).unwrap(), 90);
    assert!(
        summaries(&doc)
            .iter()
            .any(|a| a.contents.as_deref() == Some("Check figure"))
    );
    assert!(summaries(&doc).iter().any(|a| a.subtype == "Highlight"));
    assert_eq!(fs::read(&source).unwrap(), source_pdf());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn command_workflow_fills_supported_form_fields() {
    let root = std::env::temp_dir().join(format!("pdfstrike-form-cli-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("form.pdf");
    let project = root.join("form.pdfstrike");
    let output = root.join("filled.pdf");
    let mut doc = Document::new_empty();
    insert_blank_page(&mut doc, 0, 612.0, 792.0).unwrap();
    add_field(
        &mut doc,
        0,
        [72.0, 680.0, 300.0, 710.0],
        &NewField::Text { multiline: false },
        Some("contact_name"),
    )
    .unwrap();
    add_field(
        &mut doc,
        0,
        [72.0, 630.0, 96.0, 654.0],
        &NewField::CheckBox,
        Some("approved"),
    )
    .unwrap();
    fs::write(&source, write_full(&doc, &SaveOptions::default()).unwrap()).unwrap();
    let bin = env!("CARGO_BIN_EXE_pdfstrike");
    let s = source.to_str().unwrap();
    let p = project.to_str().unwrap();
    let o = output.to_str().unwrap();
    for args in [
        vec!["new", s, p],
        vec!["field-text", p, "contact_name", "Ada Lovelace"],
        vec!["field-check", p, "approved", "true"],
        vec!["export", p, o],
    ] {
        let result = Command::new(bin).args(args).output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let exported = Document::open(std::sync::Arc::new(fs::read(output).unwrap())).unwrap();
    let values = fields(&exported);
    assert_eq!(
        values
            .iter()
            .find(|f| f.name == "contact_name")
            .unwrap()
            .value,
        ["Ada Lovelace"]
    );
    assert!(
        !values
            .iter()
            .find(|f| f.name == "approved")
            .unwrap()
            .value
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}
