//! PDFStrike project controls around the pinned PdfCraft renderer and editor shell.
//! A project preview is read-only in the donor shell. Commands here are the only
//! operations written to the `.pdfstrike` journal, so its history stays truthful.

use std::path::PathBuf;

use eframe::egui;
use pdfcraft_ui_egui::{Mode, PdfCraftApp};
use pdfstrike_project::{Edit, Project};

pub struct PdfStrikeNative {
    inner: PdfCraftApp,
    project: Option<Project>,
    project_path: Option<PathBuf>,
    project_tab: Option<u64>,
    pages: usize,
    page: usize,
    destination: usize,
    note_text: String,
    field_name: String,
    field_value: String,
    x: f64,
    y: f64,
    x1: f64,
    y1: f64,
    section: Section,
    message: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Pages,
    Comments,
    Forms,
}

impl PdfStrikeNative {
    pub fn new(inner: PdfCraftApp) -> Self {
        Self {
            inner,
            project: None,
            project_path: None,
            project_tab: None,
            pages: 0,
            page: 1,
            destination: 1,
            note_text: String::new(),
            field_name: String::new(),
            field_value: String::new(),
            x: 72.0,
            y: 620.0,
            x1: 190.0,
            y1: 640.0,
            section: Section::Pages,
            message: None,
        }
    }

    pub fn open_project(&mut self, path: PathBuf) {
        let result = (|| -> Result<(), String> {
            let project = Project::open_project(&path)?;
            let pages = project.page_count()?;
            let bytes = project.export_pdf()?;
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            self.replace_preview(&name, bytes)?;
            self.project = Some(project);
            self.project_path = Some(path);
            self.pages = pages;
            self.page = 1;
            self.destination = 1;
            Ok(())
        })();
        self.report(result);
    }

    fn replace_preview(&mut self, name: &str, bytes: Vec<u8>) -> Result<(), String> {
        self.inner.open_bytes(name, None, bytes)?;
        let new_id = self.inner.active_ids().ok_or("preview did not open")?.1.0;
        if let Some(old) = self.project_tab
            && let Some(index) = self.inner.views.iter().position(|v| v.id.0 == old)
        {
            self.inner.close_tab(index);
        }
        self.project_tab = Some(new_id);
        self.inner.mode = Mode::Read;
        Ok(())
    }

    fn is_project_active(&self) -> bool {
        self.inner.active_ids().map(|(_, id)| id.0) == self.project_tab
    }

    fn restore_read_only_preview(&mut self, ctx: &egui::Context) {
        let changed =
            self.project_tab.and_then(|id| self.inner.session.get(pdfcraft_engine::DocId(id))).is_some_and(|doc| doc.dirty || doc.path.is_some());
        if !changed {
            return;
        }
        let Some(project) = &self.project else { return };
        let Some(path) = &self.project_path else { return };
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let preview = project.export_pdf();
        let result = preview.and_then(|bytes| self.replace_preview(&name, bytes));
        match result {
            Ok(()) => self.message = Some("Project preview is read-only. Use the Project edits panel; the archive is unchanged.".into()),
            Err(error) => self.message = Some(format!("Could not restore project preview: {error}")),
        }
        ctx.request_repaint();
    }

    fn create_from_active(&mut self) {
        let result = (|| -> Result<(), String> {
            let id = self.inner.active_ids().ok_or("open a PDF first")?.1;
            if Some(id.0) == self.project_tab {
                return Err("choose a source PDF tab".into());
            }
            let bytes = self.inner.session.get(id).ok_or("source tab has closed")?.bytes.as_ref().clone();
            let project = Project::open_pdf(bytes)?;
            let pages = project.page_count()?;
            let path = rfd::FileDialog::new().add_filter("PDFStrike project", &["pdfstrike"]).save_file().ok_or("project creation cancelled")?;
            if path.exists() {
                return Err("project already exists; choose a new name".into());
            }
            project.save_project(&path)?;
            let preview = project.export_pdf()?;
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            self.replace_preview(&name, preview)?;
            self.project = Some(project);
            self.project_path = Some(path);
            self.pages = pages;
            self.page = 1;
            self.destination = 1;
            Ok(())
        })();
        self.report(result);
    }

    fn edit(&mut self, edit: Edit) {
        let result = (|| -> Result<(), String> {
            let project = self.project.as_mut().ok_or("no project is open")?;
            project.apply(edit)?;
            let bytes = project.export_pdf()?;
            let path = self.project_path.as_ref().ok_or("project has no path")?;
            project.save_project(path)?;
            self.pages = project.page_count()?;
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            self.replace_preview(&name, bytes)
        })();
        self.report(result);
    }

    fn history(&mut self, redo: bool) {
        let result = (|| -> Result<(), String> {
            let project = self.project.as_mut().ok_or("no project is open")?;
            if redo {
                project.redo()?
            } else {
                project.undo()?
            }
            let bytes = project.export_pdf()?;
            let path = self.project_path.as_ref().ok_or("project has no path")?;
            project.save_project(path)?;
            self.pages = project.page_count()?;
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            self.replace_preview(&name, bytes)
        })();
        self.report(result);
    }

    fn export(&mut self) {
        let result = (|| -> Result<(), String> {
            let project = self.project.as_ref().ok_or("no project is open")?;
            let path = rfd::FileDialog::new().add_filter("PDF", &["pdf"]).save_file().ok_or("export cancelled")?;
            project.export_to_new_file(&path)?;
            Ok(())
        })();
        self.report(result);
    }

    fn report(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.message = Some("Ready.".into()),
            Err(e) if e.ends_with("cancelled") => {}
            Err(e) => {
                // A failed write must not leave an unsaved command looking committed.
                if let Some(path) = &self.project_path
                    && let Ok(saved) = Project::open_project(path)
                {
                    self.pages = saved.page_count().unwrap_or(0);
                    self.project = Some(saved);
                }
                self.message = Some(e);
            }
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("PDFStrike");
            ui.separator();
            if ui.button("Open project…").clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("PDFStrike project", &["pdfstrike"]).pick_file()
            {
                self.open_project(path);
            }
            if ui.add_enabled(self.inner.active_ids().is_some() && !self.is_project_active(), egui::Button::new("Create project from PDF…")).clicked()
            {
                self.create_from_active();
            }
            if let Some(path) = &self.project_path {
                ui.separator();
                ui.label(path.file_name().unwrap_or_default().to_string_lossy());
            }
        });
    }

    fn project_panel(&mut self, ui: &mut egui::Ui) {
        if self.project.is_none() {
            return;
        }
        let pages = self.pages;
        ui.heading("Project edits");
        ui.label(format!("{pages} pages · source retained"));
        ui.small("Edits in this panel save to the project. The preview stays in Read mode.");
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Undo").clicked() {
                self.history(false);
            }
            if ui.button("Redo").clicked() {
                self.history(true);
            }
            if ui.button("Export PDF…").clicked() {
                self.export();
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.section, Section::Pages, "Pages");
            ui.selectable_value(&mut self.section, Section::Comments, "Comments");
            ui.selectable_value(&mut self.section, Section::Forms, "Forms");
        });
        ui.add_space(8.0);
        if pages == 0 {
            return;
        }
        match self.section {
            Section::Pages => self.pages_ui(ui, pages),
            Section::Comments => self.comments_ui(ui, pages),
            Section::Forms => self.forms_ui(ui),
        }
        if let Some(message) = &self.message {
            ui.separator();
            ui.label(message);
        }
    }

    fn page_input(&mut self, ui: &mut egui::Ui, pages: usize) {
        self.page = self.page.clamp(1, pages);
        ui.horizontal(|ui| {
            ui.label("Page");
            ui.add(egui::DragValue::new(&mut self.page).range(1..=pages));
        });
    }

    fn pages_ui(&mut self, ui: &mut egui::Ui, pages: usize) {
        self.page_input(ui, pages);
        ui.horizontal(|ui| {
            if ui.button("Rotate left").clicked() {
                self.edit(Edit::Rotate { page: self.page - 1, degrees: -90 });
            }
            if ui.button("Rotate right").clicked() {
                self.edit(Edit::Rotate { page: self.page - 1, degrees: 90 });
            }
        });
        self.destination = self.destination.clamp(1, pages);
        ui.horizontal(|ui| {
            ui.label("Move to");
            ui.add(egui::DragValue::new(&mut self.destination).range(1..=pages));
            if ui.button("Move").clicked() {
                self.edit(Edit::MovePages { indices: vec![self.page - 1], to: self.destination - 1 });
            }
        });
        if ui.add_enabled(pages > 1, egui::Button::new("Delete page")).clicked() {
            self.edit(Edit::DeletePages { indices: vec![self.page - 1] });
            self.page = self.page.min(pages - 1);
        }
    }

    fn comments_ui(&mut self, ui: &mut egui::Ui, pages: usize) {
        self.page_input(ui, pages);
        ui.label("Coordinates in PDF points");
        ui.horizontal(|ui| {
            ui.label("X");
            ui.add(egui::DragValue::new(&mut self.x).speed(1.0));
            ui.label("Y");
            ui.add(egui::DragValue::new(&mut self.y).speed(1.0));
        });
        ui.label("Note");
        ui.text_edit_multiline(&mut self.note_text);
        if ui.button("Add note").clicked() {
            self.edit(Edit::Note { page: self.page - 1, at: [self.x, self.y], text: self.note_text.clone() });
        }
        ui.separator();
        ui.label("Highlight rectangle");
        ui.horizontal(|ui| {
            ui.label("X₂");
            ui.add(egui::DragValue::new(&mut self.x1).speed(1.0));
            ui.label("Y₂");
            ui.add(egui::DragValue::new(&mut self.y1).speed(1.0));
        });
        if ui.button("Add highlight").clicked() {
            self.edit(Edit::Highlight { page: self.page - 1, rect: [self.x, self.y, self.x1, self.y1] });
        }
    }

    fn forms_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("Existing AcroForm field name");
        ui.text_edit_singleline(&mut self.field_name);
        ui.label("Text value");
        ui.text_edit_singleline(&mut self.field_value);
        if ui.button("Set text field").clicked() {
            self.edit(Edit::TextField { name: self.field_name.clone(), value: self.field_value.clone() });
        }
        ui.horizontal(|ui| {
            if ui.button("Check box").clicked() {
                self.edit(Edit::CheckBox { name: self.field_name.clone(), checked: true });
            }
            if ui.button("Clear box").clicked() {
                self.edit(Edit::CheckBox { name: self.field_name.clone(), checked: false });
            }
        });
    }
}

impl eframe::App for PdfStrikeNative {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        eframe::App::raw_input_hook(&mut self.inner, ctx, raw);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::App::save(&mut self.inner, storage);
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.is_project_active() {
            self.inner.mode = Mode::Read;
        }
        eframe::App::logic(&mut self.inner, ctx, frame);
        if self.is_project_active() {
            self.inner.mode = Mode::Read;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let fill = ui.visuals().panel_fill;
        let frame_style = egui::Frame::NONE.fill(fill).inner_margin(egui::Margin::symmetric(12, 4));
        egui::Panel::top("pdfstrike_project_bar").exact_size(38.0).frame(frame_style).show(ui, |ui| self.header(ui));
        if self.project.is_some() {
            egui::Panel::left("pdfstrike_project_controls")
                .exact_size(292.0)
                .frame(egui::Frame::NONE.fill(fill).inner_margin(egui::Margin::same(12)))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| self.project_panel(ui));
                });
        }
        eframe::App::ui(&mut self.inner, ui, frame);
        self.restore_read_only_preview(ui.ctx());
    }
}
