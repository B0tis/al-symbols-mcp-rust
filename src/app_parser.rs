use std::io::{Cursor, Read};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("File not found in archive: {0}")]
    EntryNotFound(String),
    #[error("No ZIP signature found in .app file")]
    NoZipSignature,
}

const NAVX_HEADER_SIZE: usize = 40;
const ZIP_LOCAL_SIGNATURE: [u8; 2] = [0x50, 0x4B];

fn find_zip_start(buffer: &[u8]) -> Option<usize> {
    let search_end = buffer.len().min(200);
    for i in NAVX_HEADER_SIZE..search_end {
        if i + 1 < buffer.len()
            && buffer[i] == ZIP_LOCAL_SIGNATURE[0]
            && buffer[i + 1] == ZIP_LOCAL_SIGNATURE[1]
        {
            return Some(i);
        }
    }
    None
}

fn strip_bom_and_nulls(data: &[u8]) -> Vec<u8> {
    let start = if data.len() >= 3 && data[0] == 0xEF && data[1] == 0xBB && data[2] == 0xBF {
        3
    } else {
        0
    };
    let mut end = data.len();
    while end > start && data[end - 1] == 0 {
        end -= 1;
    }
    data[start..end].to_vec()
}

fn find_entry_in_archive(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    entry_name: &str,
) -> Result<Vec<u8>, AppParseError> {
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if file.name().eq_ignore_ascii_case(entry_name) {
            let mut contents = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut contents)?;
            return Ok(strip_bom_and_nulls(&contents));
        }
    }
    Err(AppParseError::EntryNotFound(entry_name.to_string()))
}

pub fn extract_entry_from_app(app_path: &Path, entry_name: &str) -> Result<Vec<u8>, AppParseError> {
    let buffer = std::fs::read(app_path)?;

    // Strategy 1: Extract ZIP portion from after the NAVX header and let
    // the zip crate find the EOCD itself. This handles both signed and
    // unsigned packages correctly.
    if let Some(zip_start) = find_zip_start(&buffer) {
        let zip_slice = &buffer[zip_start..];
        if let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(zip_slice)) {
            match find_entry_in_archive(&mut archive, entry_name) {
                Ok(data) => return Ok(data),
                Err(AppParseError::EntryNotFound(_)) => {}
                Err(e) => return Err(e),
            }
        }
    }

    // Strategy 2: Try the full buffer directly. Some .app files may have
    // ZIP offsets that include the NAVX header.
    if let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(buffer.as_slice())) {
        match find_entry_in_archive(&mut archive, entry_name) {
            Ok(data) => return Ok(data),
            Err(AppParseError::EntryNotFound(name)) => {
                return Err(AppParseError::EntryNotFound(name))
            }
            Err(e) => return Err(e),
        }
    }

    if find_zip_start(&buffer).is_none() {
        return Err(AppParseError::NoZipSignature);
    }

    Err(AppParseError::Zip(zip::result::ZipError::InvalidArchive(
        "Could not open ZIP archive in .app file with any strategy",
    )))
}

pub fn extract_symbol_reference(app_path: &Path) -> Result<Vec<u8>, AppParseError> {
    extract_entry_from_app(app_path, "SymbolReference.json")
}

pub fn extract_manifest(app_path: &Path) -> Result<Vec<u8>, AppParseError> {
    extract_entry_from_app(app_path, "NavxManifest.xml")
}
