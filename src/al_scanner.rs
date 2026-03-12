use regex::Regex;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct SourceObject {
    pub object_type: String,
    pub id: i64,
    pub name: String,
    pub file: String,
    pub line: usize,
}

static OBJECT_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?im)^\s*(table|tableextension|page|pageextension|report|reportextension|codeunit|xmlport|enum|enumextension|query|permissionset|permissionsetextension|profile|controladdin|interface)\s+(\d+)\s+"?([^"{\n]+)"?"#,
    )
    .unwrap()
});

pub fn scan_al_sources(
    app_dir: &Path,
    object_type_filter: Option<&str>,
) -> Vec<SourceObject> {
    let mut objects = Vec::new();
    let exclude: HashSet<&str> = [".alpackages", ".snapshots", "node_modules", ".git"]
        .iter()
        .copied()
        .collect();

    let filter_lower = object_type_filter.map(|t| normalize_object_type(t));

    for entry in WalkDir::new(app_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                !exclude.contains(name.as_ref())
            } else {
                true
            }
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !ext.eq_ignore_ascii_case("al") {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let rel_path = path
            .strip_prefix(app_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        for (line_idx, line) in content.lines().enumerate() {
            if let Some(caps) = OBJECT_DECL_RE.captures(line) {
                let obj_type_raw = caps.get(1).unwrap().as_str();
                let obj_type_normalized = normalize_object_type(obj_type_raw);

                if let Some(ref ft) = filter_lower {
                    if &obj_type_normalized != ft {
                        continue;
                    }
                }

                let id: i64 = match caps.get(2).unwrap().as_str().parse() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let name = caps.get(3).unwrap().as_str().trim().to_string();

                objects.push(SourceObject {
                    object_type: obj_type_normalized,
                    id,
                    name,
                    file: rel_path.clone(),
                    line: line_idx + 1,
                });
            }
        }
    }

    objects
}

fn normalize_object_type(raw: &str) -> String {
    match raw.to_lowercase().as_str() {
        "table" => "table",
        "tableextension" | "table extension" => "tableextension",
        "page" => "page",
        "pageextension" | "page extension" => "pageextension",
        "report" => "report",
        "reportextension" | "report extension" => "reportextension",
        "codeunit" => "codeunit",
        "xmlport" => "xmlport",
        "enum" => "enum",
        "enumextension" | "enum extension" | "enumextensiontype" => "enumextension",
        "query" => "query",
        "permissionset" | "permission set" => "permissionset",
        "permissionsetextension" | "permission set extension" => "permissionsetextension",
        "profile" => "profile",
        "controladdin" | "control addin" | "control add-in" => "controladdin",
        "interface" => "interface",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_parse_object_declarations() {
        let dir = std::env::temp_dir().join("al_scanner_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("MyTable.al"),
            r#"table 70000 "My Custom Table"
{
    fields
    {
        field(1; "No."; Code[20]) { }
        field(2; "Name"; Text[100]) { }
    }
}
"#,
        )
        .unwrap();

        fs::write(
            dir.join("MyPage.al"),
            r#"page 70001 "My Custom Page"
{
    SourceTable = "My Custom Table";
}
"#,
        )
        .unwrap();

        fs::write(
            dir.join("MyExtension.al"),
            r#"tableextension 70002 "Customer Ext" extends Customer
{
    fields
    {
        field(50000; "My Field"; Boolean) { }
    }
}
"#,
        )
        .unwrap();

        // This should be excluded
        let pkg_dir = dir.join(".alpackages");
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(
            pkg_dir.join("Dependency.al"),
            "table 18 \"Customer\"\n{\n}\n",
        )
        .unwrap();

        let all = scan_al_sources(&dir, None);
        assert_eq!(all.len(), 3);

        let tables = scan_al_sources(&dir, Some("table"));
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].id, 70000);
        assert_eq!(tables[0].name, "My Custom Table");

        let pages = scan_al_sources(&dir, Some("page"));
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].id, 70001);

        let exts = scan_al_sources(&dir, Some("tableextension"));
        assert_eq!(exts.len(), 1);
        assert_eq!(exts[0].id, 70002);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_normalize_object_type() {
        assert_eq!(normalize_object_type("Table"), "table");
        assert_eq!(normalize_object_type("CODEUNIT"), "codeunit");
        assert_eq!(normalize_object_type("tableextension"), "tableextension");
        assert_eq!(normalize_object_type("PageExtension"), "pageextension");
        assert_eq!(normalize_object_type("EnumExtensionType"), "enumextension");
    }
}
