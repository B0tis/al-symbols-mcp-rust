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
const ZIP_EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4B, 0x05, 0x06];
const ZIP_LOCAL_SIGNATURE: [u8; 2] = [0x50, 0x4B];

fn find_zip_start(buffer: &[u8]) -> Option<usize> {
    let search_end = buffer.len().min(200);
    for i in NAVX_HEADER_SIZE..search_end {
        if i + 1 < buffer.len() && buffer[i] == ZIP_LOCAL_SIGNATURE[0] && buffer[i + 1] == ZIP_LOCAL_SIGNATURE[1] {
            return Some(i);
        }
    }
    None
}

fn find_zip_end(buffer: &[u8]) -> Option<usize> {
    if buffer.len() < 22 {
        return None;
    }
    let search_start = if buffer.len() > 65557 {
        buffer.len() - 65557
    } else {
        0
    };
    for i in (search_start..buffer.len() - 3).rev() {
        if buffer[i..i + 4] == ZIP_EOCD_SIGNATURE {
            let comment_len = if i + 20 < buffer.len() {
                u16::from_le_bytes([buffer[i + 20], buffer[i + 21]]) as usize
            } else {
                0
            };
            return Some(i + 22 + comment_len);
        }
    }
    None
}

fn extract_zip_portion(buffer: &[u8]) -> Result<Vec<u8>, AppParseError> {
    let zip_start = find_zip_start(buffer).ok_or(AppParseError::NoZipSignature)?;
    let zip_end = find_zip_end(buffer).unwrap_or(buffer.len());
    Ok(buffer[zip_start..zip_end].to_vec())
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

pub fn extract_entry_from_app(app_path: &Path, entry_name: &str) -> Result<Vec<u8>, AppParseError> {
    let buffer = std::fs::read(app_path)?;
    let zip_data = extract_zip_portion(&buffer)?;
    let cursor = Cursor::new(zip_data);
    let mut archive = zip::ZipArchive::new(cursor)?;

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

pub fn extract_symbol_reference(app_path: &Path) -> Result<Vec<u8>, AppParseError> {
    extract_entry_from_app(app_path, "SymbolReference.json")
}

pub fn extract_manifest(app_path: &Path) -> Result<Vec<u8>, AppParseError> {
    extract_entry_from_app(app_path, "NavxManifest.xml")
}
