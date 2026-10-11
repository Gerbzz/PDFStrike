use std::path::Path;
use std::sync::Arc;

use pdfcraft_annot::{Markup, Meta, NOTE_SIZE, NewAnnotation, NoteIcon, Shape, Style, add_annotation, rect_quad};
use pdfcraft_cos::{Document, SaveOptions, write_incremental};
use pdfcraft_forms::{FieldKind, FieldValue, fields, set_value};
use pdfcraft_organize::{delete_pages, move_pages, page_boxes, page_count, rotate_pages};
use serde::{Deserialize, Serialize};

use crate::store;

pub(crate) const MAX_PDF_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const MAX_EDITS: usize = 10_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Edit {
    MovePages { indices: Vec<usize>, to: usize },
    DeletePages { indices: Vec<usize> },
    Rotate { page: usize, degrees: i64 },
    Note { page: usize, at: [f64; 2], text: String },
    Highlight { page: usize, rect: [f64; 4] },
    TextField { name: String, value: String },
    CheckBox { name: String, checked: bool },
}

/// The original PDF is immutable. Supported edits are replayed from a bounded command list.
pub struct Project {
    pub(crate) source: Vec<u8>,
    pub(crate) edits: Vec<Edit>,
    pub(crate) cursor: usize,
}

impl Project {
    pub fn open_pdf(source: Vec<u8>) -> Result<Self, String> {
        if source.is_empty() || source.len() > MAX_PDF_BYTES {
            return Err("PDF source size is outside the supported 1-byte to 128-MiB range".into());
        }
        let doc = Document::open(Arc::new(source.clone())).map_err(|e| format!("cannot open PDF: {e}"))?;
        if doc.security().is_some() {
            return Err("encrypted PDFs are not editable in this release".into());
        }
        let certified = doc
            .root()
            .and_then(|root| doc.get(root).as_dict().and_then(|catalog| catalog.get(b"Perms").cloned()))
            .map(|perms| doc.resolve(&perms))
            .and_then(|perms| perms.as_dict().cloned())
            .is_some_and(|perms| perms.contains(b"DocMDP") || perms.contains(b"UR3"));
        if certified || fields(&doc).iter().any(|field| field.kind == FieldKind::Signature) {
            return Err("digitally signed or signature-field PDFs are read-only for PDFStrike projects; editing or exporting a new PDF could invalidate a signature".into());
        }
        if page_count(&doc).map_err(|e| e.to_string())? == 0 {
            return Err("PDF has no pages".into());
        }
        Ok(Self { source, edits: Vec::new(), cursor: 0 })
    }

    pub fn source_bytes(&self) -> &[u8] {
        &self.source
    }

    pub fn page_count(&self) -> Result<usize, String> {
        page_count(&self.document()?).map_err(|e| e.to_string())
    }

    pub fn apply(&mut self, edit: Edit) -> Result<(), String> {
        if self.cursor >= MAX_EDITS {
            return Err("project edit limit reached".into());
        }
        let mut candidate = self.edits[..self.cursor].to_vec();
        candidate.push(edit);
        Self::document_for(&self.source, &candidate)?;
        self.edits = candidate;
        self.cursor = self.edits.len();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), String> {
        if self.cursor == 0 {
            return Err("there is no edit to undo".into());
        }
        self.cursor -= 1;
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), String> {
        if self.cursor == self.edits.len() {
            return Err("there is no edit to redo".into());
        }
        self.cursor += 1;
        Ok(())
    }

    pub fn export_pdf(&self) -> Result<Vec<u8>, String> {
        let doc = self.document()?;
        let expected_pages = page_count(&doc).map_err(|e| e.to_string())?;
        let bytes = write_incremental(&doc, &SaveOptions::default()).map_err(|e| format!("PDF write failed: {e}"))?;
        let reopened = Document::open(Arc::new(bytes.clone())).map_err(|e| format!("exported PDF did not reopen: {e}"))?;
        let actual_pages = page_count(&reopened).map_err(|e| e.to_string())?;
        if actual_pages != expected_pages {
            return Err(format!("exported PDF has {actual_pages} pages; expected {expected_pages}"));
        }
        Ok(bytes)
    }

    pub fn save_project(&self, path: &Path) -> Result<(), String> {
        store::save(self, path)
    }

    /// Stage and sync the complete PDF, then create a new destination without overwriting.
    pub fn export_to_new_file(&self, path: &Path) -> Result<(), String> {
        store::export_new(self, path)
    }

    pub fn open_project(path: &Path) -> Result<Self, String> {
        store::open(path)
    }

    fn document(&self) -> Result<Document, String> {
        Self::document_for(&self.source, &self.edits[..self.cursor])
    }

    pub(crate) fn document_for(source: &[u8], edits: &[Edit]) -> Result<Document, String> {
        let mut doc = Document::open(Arc::new(source.to_vec())).map_err(|e| e.to_string())?;
        for (i, edit) in edits.iter().enumerate() {
            match edit {
                Edit::MovePages { indices, to } => {
                    let unique = indices.iter().copied().collect::<std::collections::HashSet<_>>();
                    if indices.is_empty() || unique.len() != indices.len() || *to > page_count(&doc).map_err(|e| e.to_string())? {
                        return Err("move needs unique page indices and a destination within the document".into());
                    }
                    move_pages(&mut doc, indices, *to).map_err(|e| e.to_string())?;
                }
                Edit::DeletePages { indices } => {
                    let unique = indices.iter().copied().collect::<std::collections::HashSet<_>>();
                    if indices.is_empty() || unique.len() != indices.len() {
                        return Err("delete needs unique page indices".into());
                    }
                    delete_pages(&mut doc, indices).map_err(|e| e.to_string())?;
                }
                Edit::Rotate { page, degrees } => {
                    if degrees.rem_euclid(90) != 0 {
                        return Err("rotation must be a multiple of 90 degrees".into());
                    }
                    rotate_pages(&mut doc, &[*page], *degrees).map_err(|e| e.to_string())?;
                }
                Edit::Note { page, at, text } => {
                    if !at.iter().all(|v| v.is_finite()) || text.trim().is_empty() || text.len() > 8192 {
                        return Err("note needs finite coordinates and 1 to 8192 bytes of text".into());
                    }
                    let crop = page_boxes(&doc).map_err(|e| e.to_string())?.get(*page).ok_or("note page does not exist")?[1];
                    if at[0] < crop[0] || at[0] + NOTE_SIZE > crop[2] || at[1] - NOTE_SIZE < crop[1] || at[1] > crop[3] {
                        return Err("note icon must fit inside the page crop box".into());
                    }
                    let shape = Shape::Note { at: *at, icon: NoteIcon::Comment };
                    let note =
                        NewAnnotation { page: *page, style: Style::default_for(&shape), shape, contents: text.clone(), author: "PDFStrike".into() };
                    add_annotation(&mut doc, &note, &Meta { date: None, id: format!("pdfstrike-{i}") }).map_err(|e| e.to_string())?;
                }
                Edit::Highlight { page, rect } => {
                    if !rect.iter().all(|v| v.is_finite()) || rect[2] <= rect[0] || rect[3] <= rect[1] {
                        return Err("highlight rectangle must have finite, increasing coordinates".into());
                    }
                    let crop = page_boxes(&doc).map_err(|e| e.to_string())?.get(*page).ok_or("highlight page does not exist")?[1];
                    if rect[0] < crop[0] || rect[1] < crop[1] || rect[2] > crop[2] || rect[3] > crop[3] {
                        return Err("highlight rectangle must fit inside the page crop box".into());
                    }
                    let shape = Shape::TextMarkup { kind: Markup::Highlight, quads: vec![rect_quad(*rect)] };
                    let note =
                        NewAnnotation { page: *page, style: Style::default_for(&shape), shape, contents: String::new(), author: "PDFStrike".into() };
                    add_annotation(&mut doc, &note, &Meta { date: None, id: format!("pdfstrike-{i}") }).map_err(|e| e.to_string())?;
                }
                Edit::TextField { name, value } => {
                    if value.len() > 8192 {
                        return Err("text field value exceeds 8192 bytes".into());
                    }
                    let field = fields(&doc).into_iter().find(|f| f.name == *name).ok_or_else(|| format!("form field {name:?} does not exist"))?;
                    if field.kind != FieldKind::Text {
                        return Err(format!("{name:?} is not a text field"));
                    }
                    set_value(&mut doc, name, &FieldValue::Text(value.clone())).map_err(|e| e.to_string())?;
                }
                Edit::CheckBox { name, checked } => {
                    let field = fields(&doc).into_iter().find(|f| f.name == *name).ok_or_else(|| format!("form field {name:?} does not exist"))?;
                    if field.kind != FieldKind::CheckBox {
                        return Err(format!("{name:?} is not a check box"));
                    }
                    set_value(&mut doc, name, &FieldValue::Check(*checked)).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(doc)
    }
}
