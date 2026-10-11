use std::fs;
use std::io::Write;

use pdfcraft_cos::{Dict, Document, Object, SaveOptions, write_full};
use pdfcraft_forms::{NewField, add_field};
use pdfcraft_organize::insert_blank_page;
use pdfstrike_project::Project;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

fn one_page() -> Vec<u8> {
    let mut doc = Document::new_empty();
    insert_blank_page(&mut doc, 0, 400.0, 600.0).unwrap();
    write_full(&doc, &SaveOptions::default()).unwrap()
}

#[test]
fn certified_pdf_cannot_become_an_editable_project() {
    let mut doc = Document::open(std::sync::Arc::new(one_page())).unwrap();
    let root = doc.root().unwrap();
    let mut catalog = doc.get(root).as_dict().unwrap().clone();
    let mut perms = Dict::new();
    let mut signature = Dict::new();
    signature.set(b"Type".to_vec(), Object::Name(b"Sig".to_vec()));
    signature.set(b"ByteRange".to_vec(), Object::Array(vec![0.into(), 10.into(), 20.into(), 30.into()]));
    perms.set(b"DocMDP".to_vec(), Object::Dict(signature));
    catalog.set(b"Perms".to_vec(), Object::Dict(perms));
    doc.set(root, Object::Dict(catalog));
    let signed = write_full(&doc, &SaveOptions::default()).unwrap();
    let error = match Project::open_pdf(signed) {
        Ok(_) => panic!("certified PDF became editable"),
        Err(error) => error,
    };
    assert!(error.contains("sign"), "{error}");
}

#[test]
fn signature_field_pdf_is_not_treated_as_an_ordinary_form() {
    let mut doc = Document::open(std::sync::Arc::new(one_page())).unwrap();
    add_field(&mut doc, 0, [72.0, 500.0, 250.0, 540.0], &NewField::Signature, Some("approval")).unwrap();
    let signed_form = write_full(&doc, &SaveOptions::default()).unwrap();
    let result = Project::open_pdf(signed_form);
    assert!(result.is_err());
    assert!(result.err().unwrap().contains("signature"));
}

#[test]
fn stale_staging_file_cannot_block_a_later_project_save() {
    let root = std::env::temp_dir().join(format!("pdfstrike-stale-stage-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("work.pdfstrike");
    let old_stage = path.with_extension(format!("pdfstrike.{}.tmp", std::process::id()));
    fs::write(&old_stage, b"stale save from an interrupted process").unwrap();
    let project = Project::open_pdf(one_page()).unwrap();
    project.save_project(&path).unwrap();
    assert_eq!(Project::open_project(&path).unwrap().page_count().unwrap(), 1);
    assert_eq!(fs::read(&old_stage).unwrap(), b"stale save from an interrupted process");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn export_only_creates_a_complete_new_file() {
    let root = std::env::temp_dir().join(format!("pdfstrike-export-new-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("review.pdf");
    let project = Project::open_pdf(one_page()).unwrap();
    project.export_to_new_file(&path).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(Document::open(std::sync::Arc::new(bytes)).unwrap().root().is_some(), true);
    assert!(project.export_to_new_file(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), project.export_pdf().unwrap());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn tampered_project_source_hash_is_rejected_before_export() {
    let root = std::env::temp_dir().join(format!("pdfstrike-tampered-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("tampered.pdfstrike");
    let mut archive = ZipWriter::new(fs::File::create(&path).unwrap());
    let options = SimpleFileOptions::default();
    archive.start_file("manifest.json", options).unwrap();
    archive.write_all(br#"{"version":1,"source_sha256":"bad","edits":[],"cursor":0}"#).unwrap();
    archive.start_file("source.pdf", options).unwrap();
    archive.write_all(&one_page()).unwrap();
    archive.finish().unwrap();
    let error = match Project::open_project(&path) {
        Ok(_) => panic!("tampered project opened"),
        Err(error) => error,
    };
    assert!(error.contains("hash"), "{error}");
    fs::remove_dir_all(root).unwrap();
}
