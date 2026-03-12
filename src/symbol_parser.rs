use crate::types::*;
use serde_json::Value;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SymbolParseError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("App parse error: {0}")]
    AppParse(#[from] crate::app_parser::AppParseError),
}

pub fn parse_symbols_from_app(
    app_path: &Path,
    package_name: &str,
) -> Result<Vec<ALObject>, SymbolParseError> {
    let data = crate::app_parser::extract_symbol_reference(app_path)?;
    let json_str = String::from_utf8_lossy(&data);
    let root: Value = serde_json::from_str(&json_str)?;
    Ok(process_symbol_reference(&root, package_name))
}

pub fn parse_symbols_from_json(
    json_data: &[u8],
    package_name: &str,
) -> Result<Vec<ALObject>, SymbolParseError> {
    let json_str = String::from_utf8_lossy(json_data);
    let root: Value = serde_json::from_str(&json_str)?;
    Ok(process_symbol_reference(&root, package_name))
}

fn process_symbol_reference(root: &Value, package_name: &str) -> Vec<ALObject> {
    let mut objects = Vec::new();

    if let Some(namespaces) = root.get("Namespaces").and_then(|v| v.as_array()) {
        for ns in namespaces {
            let ns_name = ns.get("Name").and_then(|v| v.as_str()).map(|s| s.to_string());
            process_objects_at_level(ns, package_name, &ns_name, &mut objects);
        }
    }

    process_objects_at_level(root, package_name, &None, &mut objects);

    objects
}

const OBJECT_TYPE_KEYS: &[&str] = &[
    "Tables",
    "TableExtensions",
    "Pages",
    "PageExtensions",
    "PageCustomizations",
    "Codeunits",
    "Reports",
    "ReportExtensions",
    "Queries",
    "XmlPorts",
    "EnumTypes",
    "EnumExtensionTypes",
    "Interfaces",
    "PermissionSets",
    "PermissionSetExtensions",
    "ControlAddIns",
    "Profiles",
    "Entitlements",
];

fn process_objects_at_level(
    value: &Value,
    package_name: &str,
    namespace: &Option<String>,
    objects: &mut Vec<ALObject>,
) {
    for key in OBJECT_TYPE_KEYS {
        if let Some(arr) = value.get(*key).and_then(|v| v.as_array()) {
            let obj_type = ALObjectType::from_plural(key);
            for item in arr {
                if let Some(obj) = parse_object(item, &obj_type, package_name, namespace) {
                    objects.push(obj);
                }
            }
        }
    }
}

fn parse_object(
    value: &Value,
    obj_type: &ALObjectType,
    package_name: &str,
    namespace: &Option<String>,
) -> Option<ALObject> {
    let id = value.get("Id").and_then(|v| v.as_i64()).unwrap_or(0);
    let name = value.get("Name").and_then(|v| v.as_str())?.to_string();

    let target_object = value
        .get("TargetObject")
        .and_then(|v| v.as_str())
        .map(|s| {
            // Format: #appid#ObjectName - extract last segment
            if let Some(pos) = s.rfind('#') {
                s[pos + 1..].to_string()
            } else {
                s.to_string()
            }
        });

    let source_table = value
        .get("Properties")
        .and_then(|v| v.as_array())
        .and_then(|props| {
            props.iter().find_map(|p| {
                let pname = p.get("Name").and_then(|v| v.as_str())?;
                if pname == "SourceTable" {
                    p.get("Value").and_then(|v| v.as_str()).map(|s| {
                        if let Some(pos) = s.rfind('#') {
                            s[pos + 1..].to_string()
                        } else {
                            s.to_string()
                        }
                    })
                } else {
                    None
                }
            })
        });

    let fields = parse_fields(value);
    let keys = parse_keys(value);
    let procedures = parse_procedures(value);
    let properties = parse_properties(value);
    let enum_values = parse_enum_values(value);
    let controls = parse_controls(value);
    let data_items = parse_data_items(value);
    let variables = parse_variables(value);

    Some(ALObject {
        id,
        name,
        object_type: obj_type.clone(),
        namespace: namespace.clone(),
        package_name: Some(package_name.to_string()),
        target_object,
        fields,
        keys,
        procedures,
        properties,
        enum_values,
        controls,
        data_items,
        variables,
        source_table,
    })
}

fn parse_type_definition(value: &Value) -> ALTypeDefinition {
    let name = value
        .get("Name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_string();
    let length = value.get("Length").and_then(|v| v.as_u64()).map(|v| v as u32);
    let subtype = value
        .get("Subtype")
        .or_else(|| value.get("SubtypeDefinition"))
        .map(|v| Box::new(parse_type_definition(v)));

    ALTypeDefinition {
        name,
        length,
        subtype,
    }
}

fn parse_fields(value: &Value) -> Vec<ALField> {
    value
        .get("Fields")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let id = f.get("Id").and_then(|v| v.as_i64()).unwrap_or(0);
                    let name = f.get("Name").and_then(|v| v.as_str())?.to_string();
                    let type_def = f
                        .get("TypeDefinition")
                        .map(parse_type_definition)
                        .unwrap_or(ALTypeDefinition {
                            name: "Unknown".into(),
                            length: None,
                            subtype: None,
                        });
                    let properties = parse_property_list(f);
                    Some(ALField {
                        id,
                        name,
                        type_definition: type_def,
                        properties,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_keys(value: &Value) -> Vec<ALKey> {
    value
        .get("Keys")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|k| {
                    let name = k.get("Name").and_then(|v| v.as_str())?.to_string();
                    let field_names = k
                        .get("FieldNames")
                        .and_then(|v| v.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    let properties = parse_property_list(k);
                    Some(ALKey {
                        name,
                        field_names,
                        properties,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_procedures(value: &Value) -> Vec<ALProcedure> {
    let methods = value.get("Methods").or_else(|| value.get("Procedures"));
    methods
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let name = m.get("Name").and_then(|v| v.as_str())?.to_string();
                    let return_type = m.get("ReturnTypeDefinition").map(parse_type_definition);
                    let parameters = m
                        .get("Parameters")
                        .and_then(|v| v.as_array())
                        .map(|params| {
                            params
                                .iter()
                                .filter_map(|p| {
                                    let pname =
                                        p.get("Name").and_then(|v| v.as_str())?.to_string();
                                    let type_def = p
                                        .get("TypeDefinition")
                                        .map(parse_type_definition)
                                        .unwrap_or(ALTypeDefinition {
                                            name: "Unknown".into(),
                                            length: None,
                                            subtype: None,
                                        });
                                    let is_var =
                                        p.get("IsVar").and_then(|v| v.as_bool()).unwrap_or(false);
                                    Some(ALParameter {
                                        name: pname,
                                        type_definition: type_def,
                                        is_var,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let properties = parse_property_list(m);
                    Some(ALProcedure {
                        name,
                        return_type,
                        parameters,
                        properties,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_properties(value: &Value) -> Vec<ALProperty> {
    parse_property_list(value)
}

fn parse_property_list(value: &Value) -> Vec<ALProperty> {
    value
        .get("Properties")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let name = p.get("Name").and_then(|v| v.as_str())?.to_string();
                    let val = p
                        .get("Value")
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default();
                    Some(ALProperty { name, value: val })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_enum_values(value: &Value) -> Vec<ALEnumValue> {
    value
        .get("Values")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|ev| {
                    let ordinal = ev.get("Ordinal").and_then(|v| v.as_i64()).unwrap_or(0);
                    let name = ev.get("Name").and_then(|v| v.as_str())?.to_string();
                    let properties = parse_property_list(ev);
                    Some(ALEnumValue {
                        ordinal,
                        name,
                        properties,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_controls(value: &Value) -> Vec<ALControl> {
    value
        .get("Controls")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_single_control).collect())
        .unwrap_or_default()
}

fn parse_single_control(value: &Value) -> Option<ALControl> {
    let name = value.get("Name").and_then(|v| v.as_str())?.to_string();
    let kind = value.get("Kind").and_then(|v| v.as_str()).map(String::from);
    let properties = parse_property_list(value);
    let children = value
        .get("Controls")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_single_control).collect())
        .unwrap_or_default();
    Some(ALControl {
        name,
        kind,
        properties,
        children,
    })
}

fn parse_data_items(value: &Value) -> Vec<ALDataItem> {
    value
        .get("DataItems")
        .or_else(|| value.get("Dataset"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_single_data_item).collect())
        .unwrap_or_default()
}

fn parse_single_data_item(value: &Value) -> Option<ALDataItem> {
    let name = value
        .get("Name")
        .and_then(|v| v.as_str())
        .unwrap_or("unnamed")
        .to_string();
    let table_name = value
        .get("TableName")
        .or_else(|| value.get("SourceTableName"))
        .and_then(|v| v.as_str())
        .map(|s| {
            if let Some(pos) = s.rfind('#') {
                s[pos + 1..].to_string()
            } else {
                s.to_string()
            }
        });
    let columns = value
        .get("Columns")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let cname = c.get("Name").and_then(|v| v.as_str())?.to_string();
                    let source_expression = c
                        .get("SourceExpression")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    Some(ALColumn {
                        name: cname,
                        source_expression,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let children = value
        .get("DataItems")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(parse_single_data_item).collect())
        .unwrap_or_default();
    Some(ALDataItem {
        name,
        table_name,
        columns,
        children,
    })
}

fn parse_variables(value: &Value) -> Vec<ALVariable> {
    value
        .get("Variables")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|var| {
                    let name = var.get("Name").and_then(|v| v.as_str())?.to_string();
                    let type_def = var
                        .get("TypeDefinition")
                        .map(parse_type_definition)
                        .unwrap_or(ALTypeDefinition {
                            name: "Unknown".into(),
                            length: None,
                            subtype: None,
                        });
                    Some(ALVariable {
                        name,
                        type_definition: type_def,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_namespaced_symbols() {
        let json = r##"{
            "RuntimeVersion": "24.0.0.0",
            "Namespaces": [
                {
                    "Name": "Microsoft.Sales.Customer",
                    "Tables": [
                        {
                            "Id": 18,
                            "Name": "Customer",
                            "Fields": [
                                {
                                    "Id": 1,
                                    "Name": "No.",
                                    "TypeDefinition": { "Name": "Code", "Length": 20 }
                                },
                                {
                                    "Id": 2,
                                    "Name": "Name",
                                    "TypeDefinition": { "Name": "Text", "Length": 100 }
                                }
                            ],
                            "Keys": [
                                { "Name": "PK", "FieldNames": ["No."] }
                            ],
                            "Methods": [
                                {
                                    "Name": "GetFullName",
                                    "ReturnTypeDefinition": { "Name": "Text" },
                                    "Parameters": []
                                }
                            ]
                        }
                    ],
                    "Pages": [
                        {
                            "Id": 21,
                            "Name": "Customer Card",
                            "Properties": [
                                { "Name": "SourceTable", "Value": "#appid#Customer" }
                            ]
                        }
                    ],
                    "EnumTypes": [
                        {
                            "Id": 100,
                            "Name": "Customer Type",
                            "Values": [
                                { "Ordinal": 0, "Name": "Person" },
                                { "Ordinal": 1, "Name": "Company" }
                            ]
                        }
                    ]
                }
            ]
        }"##;

        let objects = parse_symbols_from_json(json.as_bytes(), "TestApp v1.0").unwrap();

        assert_eq!(objects.len(), 3);

        let table = objects.iter().find(|o| o.name == "Customer").unwrap();
        assert_eq!(table.id, 18);
        assert_eq!(table.object_type, crate::types::ALObjectType::Table);
        assert_eq!(table.namespace.as_deref(), Some("Microsoft.Sales.Customer"));
        assert_eq!(table.fields.len(), 2);
        assert_eq!(table.fields[0].name, "No.");
        assert_eq!(table.keys.len(), 1);
        assert_eq!(table.procedures.len(), 1);
        assert_eq!(table.procedures[0].name, "GetFullName");

        let page = objects.iter().find(|o| o.name == "Customer Card").unwrap();
        assert_eq!(page.id, 21);
        assert_eq!(page.object_type, crate::types::ALObjectType::Page);
        assert_eq!(page.source_table.as_deref(), Some("Customer"));

        let enum_obj = objects.iter().find(|o| o.name == "Customer Type").unwrap();
        assert_eq!(enum_obj.object_type, crate::types::ALObjectType::Enum);
        assert_eq!(enum_obj.enum_values.len(), 2);
    }

    #[test]
    fn test_parse_legacy_flat_format() {
        let json = r##"{
            "RuntimeVersion": "14.0.0.0",
            "Tables": [
                {
                    "Id": 18,
                    "Name": "Customer",
                    "Fields": [
                        { "Id": 1, "Name": "No.", "TypeDefinition": { "Name": "Code" } }
                    ]
                }
            ],
            "Codeunits": [
                {
                    "Id": 80,
                    "Name": "Sales-Post",
                    "Methods": [
                        { "Name": "Run", "Parameters": [] }
                    ]
                }
            ]
        }"##;

        let objects = parse_symbols_from_json(json.as_bytes(), "Legacy v1.0").unwrap();
        assert_eq!(objects.len(), 2);

        let table = objects.iter().find(|o| o.name == "Customer").unwrap();
        assert!(table.namespace.is_none());

        let cu = objects.iter().find(|o| o.name == "Sales-Post").unwrap();
        assert_eq!(cu.object_type, crate::types::ALObjectType::Codeunit);
        assert_eq!(cu.procedures.len(), 1);
    }

    #[test]
    fn test_parse_extension_target_object() {
        let json = r##"{
            "RuntimeVersion": "24.0.0.0",
            "TableExtensions": [
                {
                    "Id": 50000,
                    "Name": "Customer Ext",
                    "TargetObject": "#63ca2fa4-4f03-4f2b-a480-172fef340d3f#Customer",
                    "Fields": [
                        { "Id": 50000, "Name": "Custom Field", "TypeDefinition": { "Name": "Boolean" } }
                    ]
                }
            ]
        }"##;

        let objects = parse_symbols_from_json(json.as_bytes(), "ExtApp v1.0").unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].target_object.as_deref(), Some("Customer"));
        assert_eq!(objects[0].fields.len(), 1);
    }
}
