use crate::types::{ALPackageDependency, ALPackageInfo};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ManifestError {
    #[error("XML parse error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("App parse error: {0}")]
    AppParse(#[from] crate::app_parser::AppParseError),
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Missing required manifest field: {0}")]
    MissingField(String),
}

pub fn parse_manifest_from_app(app_path: &Path) -> Result<ALPackageInfo, ManifestError> {
    let xml_data = crate::app_parser::extract_manifest(app_path)?;
    let xml_str = std::str::from_utf8(&xml_data)?;
    parse_manifest_xml(xml_str, app_path.to_string_lossy().to_string())
}

fn parse_manifest_xml(xml: &str, file_path: String) -> Result<ALPackageInfo, ManifestError> {
    let mut reader = Reader::from_str(xml);

    let mut app_id = String::new();
    let mut app_name = String::new();
    let mut app_publisher = String::new();
    let mut app_version = String::new();
    let mut dependencies = Vec::new();
    let mut in_dependencies = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let local_name = e.local_name();
                let tag_name = std::str::from_utf8(local_name.as_ref()).unwrap_or("");

                match tag_name {
                    "App" => {
                        for attr in e.attributes().flatten() {
                            let local = attr.key.local_name();
                            let key = std::str::from_utf8(local.as_ref()).unwrap_or("");
                            let val = attr.unescape_value().unwrap_or_default();
                            match key {
                                "Id" | "id" => app_id = val.to_string(),
                                "Name" | "name" => app_name = val.to_string(),
                                "Publisher" | "publisher" => app_publisher = val.to_string(),
                                "Version" | "version" => app_version = val.to_string(),
                                _ => {}
                            }
                        }
                    }
                    "Dependencies" => {
                        in_dependencies = true;
                    }
                    "Dependency" if in_dependencies => {
                        let mut dep = ALPackageDependency {
                            id: String::new(),
                            name: String::new(),
                            publisher: String::new(),
                            version: String::new(),
                        };
                        for attr in e.attributes().flatten() {
                            let local = attr.key.local_name();
                            let key = std::str::from_utf8(local.as_ref()).unwrap_or("");
                            let val = attr.unescape_value().unwrap_or_default();
                            match key {
                                "Id" | "id" | "AppId" | "appId" => dep.id = val.to_string(),
                                "Name" | "name" => dep.name = val.to_string(),
                                "Publisher" | "publisher" => dep.publisher = val.to_string(),
                                "MinVersion" | "minVersion" | "Version" | "version" => {
                                    dep.version = val.to_string()
                                }
                                _ => {}
                            }
                        }
                        dependencies.push(dep);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                let tag_name = std::str::from_utf8(local.as_ref()).unwrap_or("");
                if tag_name == "Dependencies" {
                    in_dependencies = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(ManifestError::Xml(e)),
            _ => {}
        }
    }

    if app_name.is_empty() {
        return Err(ManifestError::MissingField("Name".into()));
    }

    Ok(ALPackageInfo {
        name: app_name,
        id: app_id,
        version: app_version,
        publisher: app_publisher,
        dependencies,
        file_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
        <Package xmlns="http://schemas.microsoft.com/navx/2015/packages">
          <App Id="63ca2fa4-4f03-4f2b-a480-172fef340d3f"
               Name="System Application"
               Publisher="Microsoft"
               Version="26.0.0.0" />
          <Dependencies>
            <Dependency Id="f838b087-1e0d-4252-854d-f7eb23dc378a"
                        Name="System"
                        Publisher="Microsoft"
                        MinVersion="26.0.0.0" />
          </Dependencies>
        </Package>"#;

        let info = parse_manifest_xml(xml, "test.app".into()).unwrap();
        assert_eq!(info.name, "System Application");
        assert_eq!(info.publisher, "Microsoft");
        assert_eq!(info.dependencies.len(), 1);
        assert_eq!(info.dependencies[0].name, "System");
    }
}
