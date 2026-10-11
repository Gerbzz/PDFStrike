use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::project::{Edit, MAX_EDITS, MAX_PDF_BYTES, Project};

const MAX_MANIFEST_BYTES: usize = 4 * 1024 * 1024;
static SAVE_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    source_sha256: String,
    edits: Vec<Edit>,
    cursor: usize,
}

pub(crate) fn save(project: &Project, path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or("project path has no parent directory")?;
    if !parent.is_dir() {
        return Err("project parent directory does not exist".into());
    }
    let manifest = Manifest { version: 1, source_sha256: hash(&project.source), edits: project.edits.clone(), cursor: project.cursor };
    let json = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
    if json.len() > MAX_MANIFEST_BYTES {
        return Err("project manifest exceeds 4 MiB".into());
    }
    let (temp, file) = staged_file(path)?;
    let write_result = (|| -> Result<(), String> {
        let mut zip = ZipWriter::new(file);
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("manifest.json", opts).map_err(|e| e.to_string())?;
        zip.write_all(&json).map_err(|e| e.to_string())?;
        zip.start_file("source.pdf", opts).map_err(|e| e.to_string())?;
        zip.write_all(&project.source).map_err(|e| e.to_string())?;
        zip.finish().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("cannot replace project with staged save: {error}"));
    }
    Ok(())
}

pub(crate) fn export_new(project: &Project, path: &Path) -> Result<(), String> {
    let bytes = project.export_pdf()?;
    let (temp, mut file) = staged_file(path)?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temp);
        return Err(format!("cannot stage PDF export: {error}"));
    }
    drop(file);
    // A hard link creates the destination atomically and refuses to replace an existing file.
    // If the target filesystem does not support links, refuse the export instead of leaving a
    // partially written PDF at the requested path.
    if let Err(error) = fs::hard_link(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("cannot create new PDF export destination: {error}"));
    }
    let _ = fs::remove_file(&temp);
    Ok(())
}

fn staged_file(path: &Path) -> Result<(PathBuf, File), String> {
    let tick = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let nonce = SAVE_NONCE.fetch_add(1, Ordering::Relaxed);
    (0..32)
        .find_map(|attempt| {
            let temp = path.with_extension(format!("pdfstrike.{}.{tick}.{nonce}.{attempt}.tmp", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&temp) {
                Ok(file) => Some(Ok((temp, file))),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(format!("cannot stage project save: {e}"))),
            }
        })
        .ok_or("could not reserve a unique staged save filename")?
}

pub(crate) fn open(path: &Path) -> Result<Project, String> {
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > (MAX_PDF_BYTES + MAX_MANIFEST_BYTES + 1024 * 1024) as u64 {
        return Err("project archive is too large".into());
    }
    let mut zip = ZipArchive::new(File::open(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if zip.len() != 2 {
        return Err("project archive must contain only manifest.json and source.pdf".into());
    }
    let mut json = Vec::new();
    let mut source = Vec::new();
    let mut seen_manifest = false;
    let mut seen_source = false;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.into_owned();
        let (target, limit) = match name.as_str() {
            "manifest.json" if !seen_manifest => {
                seen_manifest = true;
                (&mut json, MAX_MANIFEST_BYTES)
            }
            "source.pdf" if !seen_source => {
                seen_source = true;
                (&mut source, MAX_PDF_BYTES)
            }
            _ => return Err("project archive has a duplicate or unsafe entry".into()),
        };
        if entry.size() > limit as u64 {
            return Err("project archive entry exceeds its size limit".into());
        }
        entry.take((limit + 1) as u64).read_to_end(target).map_err(|e| e.to_string())?;
        if target.len() > limit {
            return Err("project archive entry expanded past its size limit".into());
        }
    }
    if !seen_manifest || !seen_source {
        return Err("project archive is missing a required entry".into());
    }
    let manifest: Manifest = serde_json::from_slice(&json).map_err(|e| format!("invalid project manifest: {e}"))?;
    if manifest.version != 1 || manifest.cursor > manifest.edits.len() || manifest.edits.len() > MAX_EDITS {
        return Err("unsupported project version or edit history".into());
    }
    if hash(&source) != manifest.source_sha256 {
        return Err("project source hash does not match the manifest".into());
    }
    let mut project = Project::open_pdf(source)?;
    Project::document_for(&project.source, &manifest.edits)?;
    project.edits = manifest.edits;
    project.cursor = manifest.cursor;
    Ok(project)
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
