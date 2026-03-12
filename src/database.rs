use crate::types::*;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
struct Inner {
    all_objects: Vec<ALObject>,
    objects_by_name: HashMap<String, Vec<usize>>,
    objects_by_type: HashMap<String, Vec<usize>>,
    objects_by_type_id: HashMap<String, usize>,
    objects_by_type_name: HashMap<String, Vec<usize>>,

    fields_by_table: HashMap<String, Vec<ALField>>,
    procedures_by_object: HashMap<String, Vec<ALProcedure>>,
    extensions_by_base: HashMap<String, Vec<usize>>,

    field_references: Vec<ALFieldReference>,
    field_refs_by_target: HashMap<String, Vec<usize>>,
    field_refs_by_source: HashMap<String, Vec<usize>>,

    references: Vec<ALReference>,

    package_objects: HashMap<String, Vec<usize>>,
}

#[derive(Clone)]
pub struct SymbolDatabase {
    inner: Arc<RwLock<Inner>>,
}

impl SymbolDatabase {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner::default())),
        }
    }

    pub fn add_objects(&self, objects: Vec<ALObject>) {
        let mut db = self.inner.write();

        for obj in objects {
            let idx = db.all_objects.len();
            let name_lower = obj.name.to_lowercase();
            let type_str = obj.object_type.to_string();
            let type_id_key = obj.type_id_key();
            let type_name_key = obj.type_name_key();
            let package = obj.package_name.clone().unwrap_or_default();

            db.objects_by_name
                .entry(name_lower.clone())
                .or_default()
                .push(idx);

            db.objects_by_type
                .entry(type_str.clone())
                .or_default()
                .push(idx);

            db.objects_by_type_id.insert(type_id_key, idx);

            db.objects_by_type_name
                .entry(type_name_key)
                .or_default()
                .push(idx);

            if !package.is_empty() {
                db.package_objects
                    .entry(package.clone())
                    .or_default()
                    .push(idx);
            }

            // Index fields
            if !obj.fields.is_empty() {
                let key = format!("{}:{}", type_str, name_lower);
                db.fields_by_table
                    .entry(key)
                    .or_default()
                    .extend(obj.fields.clone());
            }

            // Index procedures
            if !obj.procedures.is_empty() {
                let key = obj.type_id_key();
                db.procedures_by_object
                    .entry(key)
                    .or_default()
                    .extend(obj.procedures.clone());
            }

            // Index extensions
            if let Some(ref target) = obj.target_object {
                let target_lower = target.to_lowercase();
                db.extensions_by_base
                    .entry(target_lower)
                    .or_default()
                    .push(idx);
            }

            // Build references from this object
            Self::build_references_for_object(&obj, idx, &mut db);

            db.all_objects.push(obj);
        }
    }

    fn build_references_for_object(obj: &ALObject, _idx: usize, db: &mut Inner) {
        let source_id = format!("{}:{}", obj.object_type, obj.id);
        let source_name = &obj.name;
        let source_type = obj.object_type.to_string();

        // Extensions reference their base object
        if let Some(ref target) = obj.target_object {
            db.references.push(ALReference {
                source_name: source_name.clone(),
                source_type: source_type.clone(),
                target_name: target.clone(),
                target_type: base_type_for_extension(&obj.object_type),
                reference_type: "extends".into(),
                package_name: obj.package_name.clone(),
                context: None,
                details: None,
            });
        }

        // Page SourceTable reference
        if let Some(ref st) = obj.source_table {
            db.references.push(ALReference {
                source_name: source_name.clone(),
                source_type: source_type.clone(),
                target_name: st.clone(),
                target_type: "Table".into(),
                reference_type: "uses".into(),
                package_name: obj.package_name.clone(),
                context: Some("SourceTable".into()),
                details: None,
            });
        }

        // Variable references
        for var in &obj.variables {
            if let Some(ref sub) = var.type_definition.subtype {
                let ref_target_type = &var.type_definition.name;
                if is_object_type_name(ref_target_type) {
                    db.references.push(ALReference {
                        source_name: source_name.clone(),
                        source_type: source_type.clone(),
                        target_name: sub.name.clone(),
                        target_type: ref_target_type.clone(),
                        reference_type: "variable".into(),
                        package_name: obj.package_name.clone(),
                        context: Some(format!("Variable: {}", var.name)),
                        details: None,
                    });
                }
            }
        }

        // Procedure parameter and return type references
        for proc in &obj.procedures {
            for param in &proc.parameters {
                if let Some(ref sub) = param.type_definition.subtype {
                    let ref_target_type = &param.type_definition.name;
                    if is_object_type_name(ref_target_type) {
                        db.references.push(ALReference {
                            source_name: source_name.clone(),
                            source_type: source_type.clone(),
                            target_name: sub.name.clone(),
                            target_type: ref_target_type.clone(),
                            reference_type: "parameter".into(),
                            package_name: obj.package_name.clone(),
                            context: Some(format!("{}({})", proc.name, param.name)),
                            details: None,
                        });
                    }
                }
            }
            if let Some(ref ret) = proc.return_type {
                if let Some(ref sub) = ret.subtype {
                    if is_object_type_name(&ret.name) {
                        db.references.push(ALReference {
                            source_name: source_name.clone(),
                            source_type: source_type.clone(),
                            target_name: sub.name.clone(),
                            target_type: ret.name.clone(),
                            reference_type: "return_type".into(),
                            package_name: obj.package_name.clone(),
                            context: Some(format!("{} return type", proc.name)),
                            details: None,
                        });
                    }
                }
            }
        }

        // Table field TableRelation references
        for field in &obj.fields {
            for prop in &field.properties {
                if prop.name == "TableRelation" {
                    let target_table = extract_table_from_relation(&prop.value);
                    if !target_table.is_empty() {
                        let fr = ALFieldReference {
                            source_object_id: source_id.clone(),
                            source_object_name: source_name.clone(),
                            source_object_type: source_type.clone(),
                            target_table_name: target_table.clone(),
                            target_field_name: field.name.clone(),
                            reference_type: "table_relation".into(),
                            context: Some(format!("Field: {}", field.name)),
                            package_name: obj.package_name.clone(),
                        };
                        let fr_idx = db.field_references.len();
                        let target_key =
                            format!("{}.{}", target_table.to_lowercase(), field.name.to_lowercase());
                        db.field_refs_by_target
                            .entry(target_key)
                            .or_default()
                            .push(fr_idx);
                        db.field_refs_by_source
                            .entry(source_id.clone())
                            .or_default()
                            .push(fr_idx);
                        db.field_references.push(fr);
                    }
                }
            }
        }
    }

    pub fn search_objects(
        &self,
        pattern: Option<&str>,
        object_type: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> (Vec<ALObject>, usize) {
        let db = self.inner.read();
        let mut results: Vec<&ALObject> = Vec::new();

        let type_filter = object_type.map(|t| ALObjectType::from_str_loose(t));

        if let Some(pat) = pattern {
            let pat_lower = pat.to_lowercase();
            let is_wildcard = pat_lower.contains('*') || pat_lower.contains('?');

            if is_wildcard {
                let regex_pat = pat_lower.replace('*', ".*").replace('?', ".");
                let re = regex::Regex::new(&format!("^{}$", regex_pat)).ok();
                for obj in &db.all_objects {
                    if let Some(ref tf) = type_filter {
                        if &obj.object_type != tf {
                            continue;
                        }
                    }
                    if let Some(ref re) = re {
                        if re.is_match(&obj.name.to_lowercase()) {
                            results.push(obj);
                        }
                    }
                }
            } else {
                for obj in &db.all_objects {
                    if let Some(ref tf) = type_filter {
                        if &obj.object_type != tf {
                            continue;
                        }
                    }
                    if obj.name.to_lowercase().contains(&pat_lower) {
                        results.push(obj);
                    }
                }
            }
        } else if let Some(ref tf) = type_filter {
            let type_str = tf.to_string();
            if let Some(indices) = db.objects_by_type.get(&type_str) {
                for &idx in indices {
                    results.push(&db.all_objects[idx]);
                }
            }
        } else {
            results = db.all_objects.iter().collect();
        }

        let total = results.len();
        let page: Vec<ALObject> = results
            .into_iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();
        (page, total)
    }

    pub fn get_object_by_type_id(&self, obj_type: &str, id: i64) -> Option<ALObject> {
        let db = self.inner.read();
        let key = format!("{}:{}", obj_type, id);
        db.objects_by_type_id
            .get(&key)
            .map(|&idx| db.all_objects[idx].clone())
    }

    pub fn get_object_by_type_name(&self, obj_type: &str, name: &str) -> Option<ALObject> {
        let db = self.inner.read();
        let key = format!("{}:{}", obj_type, name.to_lowercase());
        db.objects_by_type_name.get(&key).and_then(|indices| {
            indices
                .first()
                .map(|&idx| db.all_objects[idx].clone())
        })
    }

    pub fn get_object_by_name(&self, name: &str) -> Option<ALObject> {
        let db = self.inner.read();
        let name_lower = name.to_lowercase();
        db.objects_by_name
            .get(&name_lower)
            .and_then(|indices| indices.first().map(|&idx| db.all_objects[idx].clone()))
    }

    pub fn find_references(&self, object_name: &str, object_type: Option<&str>) -> Vec<ALReference> {
        let db = self.inner.read();
        let name_lower = object_name.to_lowercase();
        let mut results = Vec::new();

        for r in &db.references {
            if r.target_name.to_lowercase() == name_lower {
                if let Some(tf) = object_type {
                    if !r.target_type.eq_ignore_ascii_case(tf) {
                        continue;
                    }
                }
                results.push(r.clone());
            }
        }

        results
    }

    pub fn find_field_references(&self, table_name: &str, field_name: &str) -> Vec<ALFieldReference> {
        let db = self.inner.read();
        let key = format!("{}.{}", table_name.to_lowercase(), field_name.to_lowercase());
        db.field_refs_by_target
            .get(&key)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| db.field_references[idx].clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_extensions_for(&self, base_name: &str) -> Vec<ALObject> {
        let db = self.inner.read();
        let key = base_name.to_lowercase();
        db.extensions_by_base
            .get(&key)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| db.all_objects[idx].clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn search_members(
        &self,
        object_name: Option<&str>,
        object_type: Option<&str>,
        member_type: Option<&str>,
        member_pattern: Option<&str>,
        limit: usize,
    ) -> Vec<serde_json::Value> {
        let db = self.inner.read();
        let mut results = Vec::new();

        let objects: Vec<&ALObject> = if let Some(name) = object_name {
            let name_lower = name.to_lowercase();
            db.all_objects
                .iter()
                .filter(|o| {
                    let matches_name = o.name.to_lowercase().contains(&name_lower);
                    let matches_type = object_type
                        .map(|t| o.object_type.to_string().eq_ignore_ascii_case(t))
                        .unwrap_or(true);
                    matches_name && matches_type
                })
                .collect()
        } else {
            db.all_objects.iter().collect()
        };

        let member_type_lower = member_type.map(|m| m.to_lowercase());
        let pat_lower = member_pattern.map(|p| p.to_lowercase());

        for obj in objects {
            if results.len() >= limit {
                break;
            }

            let should_include_procs = member_type_lower
                .as_ref()
                .map(|m| m == "procedure" || m == "method" || m == "all")
                .unwrap_or(true);
            let should_include_fields = member_type_lower
                .as_ref()
                .map(|m| m == "field" || m == "all")
                .unwrap_or(true);
            let should_include_controls = member_type_lower
                .as_ref()
                .map(|m| m == "control" || m == "all")
                .unwrap_or(true);
            let should_include_dataitems = member_type_lower
                .as_ref()
                .map(|m| m == "dataitem" || m == "all")
                .unwrap_or(true);

            if should_include_procs {
                for proc in &obj.procedures {
                    if results.len() >= limit {
                        break;
                    }
                    if matches_pattern(&proc.name, pat_lower.as_deref()) {
                        results.push(serde_json::json!({
                            "objectName": obj.name,
                            "objectType": obj.object_type.to_string(),
                            "memberType": "Procedure",
                            "memberName": proc.name,
                            "parameters": proc.parameters.iter().map(|p| {
                                format!("{}{}: {}", if p.is_var { "var " } else { "" }, p.name, p.type_definition.name)
                            }).collect::<Vec<_>>(),
                            "returnType": proc.return_type.as_ref().map(|r| &r.name),
                            "packageName": obj.package_name,
                        }));
                    }
                }
            }

            if should_include_fields {
                for field in &obj.fields {
                    if results.len() >= limit {
                        break;
                    }
                    if matches_pattern(&field.name, pat_lower.as_deref()) {
                        results.push(serde_json::json!({
                            "objectName": obj.name,
                            "objectType": obj.object_type.to_string(),
                            "memberType": "Field",
                            "memberName": field.name,
                            "fieldId": field.id,
                            "fieldType": field.type_definition.name,
                            "packageName": obj.package_name,
                        }));
                    }
                }
            }

            if should_include_controls {
                collect_controls(&obj.controls, obj, pat_lower.as_deref(), &mut results, limit);
            }

            if should_include_dataitems {
                collect_data_items(
                    &obj.data_items,
                    obj,
                    pat_lower.as_deref(),
                    &mut results,
                    limit,
                );
            }
        }

        results
    }

    pub fn object_count(&self) -> usize {
        self.inner.read().all_objects.len()
    }

    pub fn package_names(&self) -> Vec<String> {
        self.inner.read().package_objects.keys().cloned().collect()
    }

    pub fn package_stats(&self) -> Vec<serde_json::Value> {
        let db = self.inner.read();
        db.package_objects
            .iter()
            .map(|(name, indices)| {
                let mut type_counts: HashMap<String, usize> = HashMap::new();
                for &idx in indices {
                    let obj = &db.all_objects[idx];
                    *type_counts
                        .entry(obj.object_type.to_string())
                        .or_default() += 1;
                }
                serde_json::json!({
                    "packageName": name,
                    "totalObjects": indices.len(),
                    "objectTypes": type_counts,
                })
            })
            .collect()
    }

    pub fn clear(&self) {
        let mut db = self.inner.write();
        *db = Inner::default();
    }
}

fn base_type_for_extension(ext_type: &ALObjectType) -> String {
    match ext_type {
        ALObjectType::TableExtension => "Table".into(),
        ALObjectType::PageExtension | ALObjectType::PageCustomization => "Page".into(),
        ALObjectType::ReportExtension => "Report".into(),
        ALObjectType::EnumExtension => "Enum".into(),
        ALObjectType::PermissionSetExtension => "PermissionSet".into(),
        _ => "Unknown".into(),
    }
}

fn is_object_type_name(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "record"
            | "codeunit"
            | "page"
            | "report"
            | "query"
            | "xmlport"
            | "enum"
            | "interface"
    )
}

fn extract_table_from_relation(relation: &str) -> String {
    let trimmed = relation.trim();
    if let Some(pos) = trimmed.find(|c: char| !c.is_alphanumeric() && c != ' ' && c != '_') {
        trimmed[..pos].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

fn matches_pattern(name: &str, pattern: Option<&str>) -> bool {
    match pattern {
        None => true,
        Some(pat) => {
            let name_lower = name.to_lowercase();
            if pat.contains('*') || pat.contains('?') {
                let regex_pat = pat.replace('*', ".*").replace('?', ".");
                regex::Regex::new(&format!("^{}$", regex_pat))
                    .map(|re| re.is_match(&name_lower))
                    .unwrap_or(false)
            } else {
                name_lower.contains(pat)
            }
        }
    }
}

fn collect_controls(
    controls: &[ALControl],
    obj: &ALObject,
    pattern: Option<&str>,
    results: &mut Vec<serde_json::Value>,
    limit: usize,
) {
    for ctrl in controls {
        if results.len() >= limit {
            break;
        }
        if matches_pattern(&ctrl.name, pattern) {
            results.push(serde_json::json!({
                "objectName": obj.name,
                "objectType": obj.object_type.to_string(),
                "memberType": "Control",
                "memberName": ctrl.name,
                "controlKind": ctrl.kind,
                "packageName": obj.package_name,
            }));
        }
        collect_controls(&ctrl.children, obj, pattern, results, limit);
    }
}

fn collect_data_items(
    items: &[ALDataItem],
    obj: &ALObject,
    pattern: Option<&str>,
    results: &mut Vec<serde_json::Value>,
    limit: usize,
) {
    for item in items {
        if results.len() >= limit {
            break;
        }
        if matches_pattern(&item.name, pattern) {
            results.push(serde_json::json!({
                "objectName": obj.name,
                "objectType": obj.object_type.to_string(),
                "memberType": "DataItem",
                "memberName": item.name,
                "tableName": item.table_name,
                "packageName": obj.package_name,
            }));
        }
        collect_data_items(&item.children, obj, pattern, results, limit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_object(id: i64, name: &str, obj_type: ALObjectType) -> ALObject {
        ALObject {
            id,
            name: name.into(),
            object_type: obj_type,
            namespace: None,
            package_name: Some("TestPkg v1.0".into()),
            target_object: None,
            fields: vec![],
            keys: vec![],
            procedures: vec![],
            properties: vec![],
            enum_values: vec![],
            controls: vec![],
            data_items: vec![],
            variables: vec![],
            source_table: None,
        }
    }

    #[test]
    fn test_add_and_search() {
        let db = SymbolDatabase::new();
        db.add_objects(vec![
            make_test_object(18, "Customer", ALObjectType::Table),
            make_test_object(27, "Vendor", ALObjectType::Table),
            make_test_object(21, "Customer Card", ALObjectType::Page),
        ]);

        let (results, total) = db.search_objects(Some("customer"), None, 50, 0);
        assert_eq!(total, 2);
        assert_eq!(results.len(), 2);

        let (results, total) = db.search_objects(None, Some("Table"), 50, 0);
        assert_eq!(total, 2);
        assert_eq!(results.len(), 2);

        let (results, total) = db.search_objects(Some("cust*"), Some("Table"), 50, 0);
        assert_eq!(total, 1);
        assert_eq!(results[0].name, "Customer");
    }

    #[test]
    fn test_get_by_type_id() {
        let db = SymbolDatabase::new();
        db.add_objects(vec![
            make_test_object(18, "Customer", ALObjectType::Table),
        ]);

        let obj = db.get_object_by_type_id("Table", 18);
        assert!(obj.is_some());
        assert_eq!(obj.unwrap().name, "Customer");

        let obj = db.get_object_by_type_id("Table", 999);
        assert!(obj.is_none());
    }

    #[test]
    fn test_extensions_tracking() {
        let db = SymbolDatabase::new();
        let mut ext = make_test_object(50000, "MyExtension", ALObjectType::TableExtension);
        ext.target_object = Some("Customer".into());

        db.add_objects(vec![
            make_test_object(18, "Customer", ALObjectType::Table),
            ext,
        ]);

        let extensions = db.get_extensions_for("Customer");
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].name, "MyExtension");
    }

    #[test]
    fn test_reference_tracking() {
        let db = SymbolDatabase::new();

        let mut page = make_test_object(21, "Customer Card", ALObjectType::Page);
        page.source_table = Some("Customer".into());

        db.add_objects(vec![
            make_test_object(18, "Customer", ALObjectType::Table),
            page,
        ]);

        let refs = db.find_references("Customer", None);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].reference_type, "uses");
        assert_eq!(refs[0].source_name, "Customer Card");
    }

    #[test]
    fn test_pagination() {
        let db = SymbolDatabase::new();
        let objects: Vec<ALObject> = (1..=10)
            .map(|i| make_test_object(i, &format!("Table{}", i), ALObjectType::Table))
            .collect();
        db.add_objects(objects);

        let (results, total) = db.search_objects(None, None, 3, 0);
        assert_eq!(total, 10);
        assert_eq!(results.len(), 3);

        let (results, _) = db.search_objects(None, None, 3, 8);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_object_count() {
        let db = SymbolDatabase::new();
        assert_eq!(db.object_count(), 0);
        db.add_objects(vec![
            make_test_object(1, "T1", ALObjectType::Table),
            make_test_object(2, "T2", ALObjectType::Table),
        ]);
        assert_eq!(db.object_count(), 2);
    }

    #[test]
    fn test_clear() {
        let db = SymbolDatabase::new();
        db.add_objects(vec![make_test_object(1, "T1", ALObjectType::Table)]);
        assert_eq!(db.object_count(), 1);
        db.clear();
        assert_eq!(db.object_count(), 0);
    }
}
