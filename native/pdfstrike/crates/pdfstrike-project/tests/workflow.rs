use std::sync::Arc;

use pdfcraft_annot::summaries;
use pdfcraft_cos::{Document, SaveOptions, write_full};
use pdfcraft_organize::{insert_blank_page, page_boxes, page_rotation};
use pdfstrike_project::{Edit, Project};

fn source_pdf() -> Vec<u8> {
    let mut doc = Document::new_empty();
    for width in [400.0, 500.0, 600.0] {
        insert_blank_page(&mut doc, 0, width, 700.0).unwrap();
    }
    write_full(&doc, &SaveOptions::default()).unwrap()
}

#[test]
fn organize_annotate_and_reopen_export() {
    let source = source_pdf();
    let mut project = Project::open_pdf(source.clone()).unwrap();
    project.apply(Edit::MovePages { indices: vec![0], to: 2 }).unwrap();
    project.apply(Edit::Rotate { page: 0, degrees: 90 }).unwrap();
    project.apply(Edit::Note { page: 0, at: [72.0, 620.0], text: "Check figure".into() }).unwrap();
    let output = project.export_pdf().unwrap();
    let exported = Document::open(Arc::new(output)).unwrap();
    assert_eq!(page_boxes(&exported).unwrap()[0][0][2], 500.0);
    assert_eq!(page_rotation(&exported, 0).unwrap(), 90);
    assert!(summaries(&exported).iter().any(|a| a.page == 0 && a.contents.as_deref() == Some("Check figure")));
    assert_eq!(source, project.source_bytes());
}

#[test]
fn editable_archive_reopens_with_undo_history() {
    let mut project = Project::open_pdf(source_pdf()).unwrap();
    project.apply(Edit::Rotate { page: 1, degrees: 90 }).unwrap();
    let path =
        std::env::temp_dir().join(format!("pdfstrike-{}-{}.pdfstrike", std::process::id(), std::thread::current().name().unwrap_or("workflow")));
    project.save_project(&path).unwrap();
    let mut reopened = Project::open_project(&path).unwrap();
    assert_eq!(page_rotation(&Document::open(Arc::new(reopened.export_pdf().unwrap())).unwrap(), 1).unwrap(), 90);
    reopened.undo().unwrap();
    assert_eq!(page_rotation(&Document::open(Arc::new(reopened.export_pdf().unwrap())).unwrap(), 1).unwrap(), 0);
    reopened.redo().unwrap();
    assert_eq!(page_rotation(&Document::open(Arc::new(reopened.export_pdf().unwrap())).unwrap(), 1).unwrap(), 90);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn duplicate_move_indices_are_rejected_without_changing_pages() {
    let source = source_pdf();
    let mut project = Project::open_pdf(source.clone()).unwrap();
    assert!(project.apply(Edit::MovePages { indices: vec![1, 1], to: 0 }).is_err());
    let doc = Document::open(Arc::new(project.export_pdf().unwrap())).unwrap();
    assert_eq!(page_boxes(&doc).unwrap()[0][0][2], 600.0);
    assert_eq!(project.source_bytes(), source);
}

#[test]
fn delete_and_highlight_survive_export_and_reopen() {
    let mut project = Project::open_pdf(source_pdf()).unwrap();
    project.apply(Edit::DeletePages { indices: vec![1] }).unwrap();
    project.apply(Edit::Highlight { page: 0, rect: [72.0, 610.0, 180.0, 630.0] }).unwrap();
    let doc = Document::open(Arc::new(project.export_pdf().unwrap())).unwrap();
    assert_eq!(page_boxes(&doc).unwrap().len(), 2);
    assert!(summaries(&doc).iter().any(|a| a.page == 0 && a.subtype == "Highlight" && !a.quads.is_empty()));
    assert!(project.apply(Edit::DeletePages { indices: vec![0, 1] }).is_err());
    assert_eq!(project.page_count().unwrap(), 2);
}

#[test]
fn annotation_coordinates_outside_page_are_rejected_atomically() {
    let mut project = Project::open_pdf(source_pdf()).unwrap();
    project.apply(Edit::Note { page: 0, at: [72.0, 620.0], text: "Keep this".into() }).unwrap();
    assert!(project.apply(Edit::Note { page: 0, at: [-1.0, 620.0], text: "Outside".into() }).is_err());
    assert!(project.apply(Edit::Highlight { page: 0, rect: [590.0, 610.0, 650.0, 630.0] }).is_err());
    let doc = Document::open(Arc::new(project.export_pdf().unwrap())).unwrap();
    let notes = summaries(&doc);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].contents.as_deref(), Some("Keep this"));
}
