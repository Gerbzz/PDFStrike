use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pdfstrike::{Edit, Project};

const USAGE: &str = "PDFStrike (local preview)\n\
  pdfstrike new SOURCE.pdf PROJECT.pdfstrike\n\
  pdfstrike info PROJECT.pdfstrike\n\
  pdfstrike move PROJECT.pdfstrike PAGE DESTINATION\n\
  pdfstrike delete PROJECT.pdfstrike PAGE\n\
  pdfstrike rotate PROJECT.pdfstrike PAGE DEGREES\n\
  pdfstrike note PROJECT.pdfstrike PAGE X Y TEXT\n\
  pdfstrike highlight PROJECT.pdfstrike PAGE X0 Y0 X1 Y1\n\
  pdfstrike field-text PROJECT.pdfstrike NAME VALUE\n\
  pdfstrike field-check PROJECT.pdfstrike NAME true|false\n\
  pdfstrike undo PROJECT.pdfstrike\n\
  pdfstrike redo PROJECT.pdfstrike\n\
  pdfstrike export PROJECT.pdfstrike OUTPUT.pdf\n\
Pages and destinations are 1-based; coordinates use PDF points.";

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("PDFStrike: {error}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[OsString]) -> Result<(), String> {
    let command = args
        .get(1)
        .and_then(|s| s.to_str())
        .ok_or("missing command")?;
    match command {
        "help" | "--help" | "-h" => println!("{USAGE}"),
        "new" => {
            exact(args, 4)?;
            let bytes = read_limited(&path(args, 2)?, 128 * 1024 * 1024)?;
            let project = Project::open_pdf(bytes)?;
            let dest = path(args, 3)?;
            if dest.exists() {
                return Err("project already exists; choose another destination".into());
            }
            project.save_project(&dest)?;
            println!(
                "Saved {} pages to {}",
                project.page_count()?,
                dest.display()
            );
        }
        "info" => {
            exact(args, 3)?;
            let project = Project::open_project(&path(args, 2)?)?;
            println!(
                "{} pages; source {} bytes",
                project.page_count()?,
                project.source_bytes().len()
            );
        }
        "move" => {
            exact(args, 5)?;
            let src = path(args, 2)?;
            let page = one_based(args, 3)?;
            let to = one_based(args, 4)?;
            edit(
                &src,
                Edit::MovePages {
                    indices: vec![page],
                    to,
                },
            )?;
        }
        "delete" => {
            exact(args, 4)?;
            edit(
                &path(args, 2)?,
                Edit::DeletePages {
                    indices: vec![one_based(args, 3)?],
                },
            )?;
        }
        "rotate" => {
            exact(args, 5)?;
            let src = path(args, 2)?;
            let page = one_based(args, 3)?;
            let degrees = text_arg(args, 4)?
                .parse::<i64>()
                .map_err(|_| "degrees must be an integer")?;
            edit(&src, Edit::Rotate { page, degrees })?;
        }
        "note" => {
            exact(args, 7)?;
            let src = path(args, 2)?;
            let page = one_based(args, 3)?;
            let x = text_arg(args, 4)?
                .parse::<f64>()
                .map_err(|_| "X must be a number")?;
            let y = text_arg(args, 5)?
                .parse::<f64>()
                .map_err(|_| "Y must be a number")?;
            edit(
                &src,
                Edit::Note {
                    page,
                    at: [x, y],
                    text: text_arg(args, 6)?.into(),
                },
            )?;
        }
        "highlight" => {
            exact(args, 8)?;
            let page = one_based(args, 3)?;
            let mut rect = [0.0; 4];
            for (index, value) in rect.iter_mut().enumerate() {
                *value = text_arg(args, index + 4)?
                    .parse::<f64>()
                    .map_err(|_| "highlight coordinates must be numbers")?;
            }
            edit(&path(args, 2)?, Edit::Highlight { page, rect })?;
        }
        "field-text" => {
            exact(args, 5)?;
            edit(
                &path(args, 2)?,
                Edit::TextField {
                    name: text_arg(args, 3)?.into(),
                    value: text_arg(args, 4)?.into(),
                },
            )?;
        }
        "field-check" => {
            exact(args, 5)?;
            let checked = text_arg(args, 4)?
                .parse::<bool>()
                .map_err(|_| "checkbox value must be true or false")?;
            edit(
                &path(args, 2)?,
                Edit::CheckBox {
                    name: text_arg(args, 3)?.into(),
                    checked,
                },
            )?;
        }
        "undo" | "redo" => {
            exact(args, 3)?;
            let path = path(args, 2)?;
            let mut project = Project::open_project(&path)?;
            if command == "undo" {
                project.undo()?;
            } else {
                project.redo()?;
            }
            project.save_project(&path)?;
            println!("{command} saved to {}", path.display());
        }
        "export" => {
            exact(args, 4)?;
            let project = Project::open_project(&path(args, 2)?)?;
            let dest = path(args, 3)?;
            if dest.exists() {
                return Err("output already exists; choose another destination".into());
            }
            project.export_to_new_file(&dest)?;
            println!(
                "Exported {} pages to {}",
                project.page_count()?,
                dest.display()
            );
        }
        _ => return Err(format!("unknown command: {command}")),
    }
    Ok(())
}

fn edit(path: &Path, command: Edit) -> Result<(), String> {
    let mut project = Project::open_project(path)?;
    project.apply(command)?;
    project.save_project(path)?;
    println!(
        "Saved {} pages to {}",
        project.page_count()?,
        path.display()
    );
    Ok(())
}

fn path(args: &[OsString], index: usize) -> Result<PathBuf, String> {
    args.get(index)
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing argument {index}"))
}

fn text_arg(args: &[OsString], index: usize) -> Result<&str, String> {
    args.get(index)
        .and_then(|a| a.to_str())
        .ok_or_else(|| format!("argument {index} must be text"))
}

fn one_based(args: &[OsString], index: usize) -> Result<usize, String> {
    let n = text_arg(args, index)?
        .parse::<usize>()
        .map_err(|_| format!("argument {index} must be a positive page number"))?;
    n.checked_sub(1)
        .ok_or_else(|| format!("argument {index} must be at least 1"))
}

fn exact(args: &[OsString], count: usize) -> Result<(), String> {
    if args.len() == count {
        Ok(())
    } else {
        Err(format!(
            "expected {} arguments, got {}",
            count - 1,
            args.len().saturating_sub(1)
        ))
    }
}

fn read_limited(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > max {
        return Err("source PDF exceeds the 128-MiB limit".into());
    }
    fs::read(path).map_err(|e| e.to_string())
}
