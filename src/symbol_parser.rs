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
        process_namespaces(namespaces, package_name, &mut objects);
    }

    // Legacy format: objects directly at root level (pre-namespace AL)
    process_objects_at_level(root, package_name, &None, &mut objects);

    objects
}

/// Recursively walk the namespace tree so sub-namespaces are never silently lost.
fn process_namespaces(namespaces: &[Value], package_name: &str, objects: &mut Vec<ALObject>) {
    for ns in namespaces {
        let ns_name = ns.get("Name").and_then(|v| v.as_str()).map(|s| s.to_string());
        process_objects_at_level(ns, package_name, &ns_name, objects);

        if let Some(children) = ns.get("Namespaces").and_then(|v| v.as_array()) {
            process_namespaces(children, package_name, objects);
        }
    }
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
    "Enums",
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
        .or_else(|| value.get("Elements"))
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
        .or_else(|| value.get("RelatedTable"))
        .or_else(|| value.get("SourceTable"))
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
                        .or_else(|| c.get("SourceExpr"))
                        .or_else(|| c.get("SourceColumn"))
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
        .or_else(|| value.get("Elements"))
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
    fn test_parse_nested_namespaces() {
        let json = r##"{
            "RuntimeVersion": "26.0.0.0",
            "Namespaces": [
                {
                    "Name": "Microsoft.Foundation",
                    "Tables": [
                        { "Id": 8, "Name": "Language", "Fields": [] }
                    ],
                    "Namespaces": [
                        {
                            "Name": "Microsoft.Foundation.Address",
                            "Tables": [
                                { "Id": 9, "Name": "Country/Region", "Fields": [] }
                            ]
                        }
                    ]
                },
                {
                    "Name": "Microsoft.Sales.Document",
                    "Tables": [
                        {
                            "Id": 36,
                            "Name": "Sales Header",
                            "Fields": [
                                { "Id": 1, "Name": "Document Type", "TypeDefinition": { "Name": "Enum" } },
                                { "Id": 2, "Name": "Sell-to Customer No.", "TypeDefinition": { "Name": "Code", "Length": 20 } },
                                { "Id": 3, "Name": "No.", "TypeDefinition": { "Name": "Code", "Length": 20 } }
                            ],
                            "Keys": [
                                { "Name": "PK", "FieldNames": ["Document Type", "No."] }
                            ],
                            "Methods": [
                                { "Name": "InitRecord", "Parameters": [] },
                                { "Name": "AssistEdit", "ReturnTypeDefinition": { "Name": "Boolean" }, "Parameters": [
                                    { "Name": "OldSalesHeader", "TypeDefinition": { "Name": "Record", "Subtype": { "Name": "Sales Header" } }, "IsVar": false }
                                ]}
                            ]
                        },
                        {
                            "Id": 37,
                            "Name": "Sales Line",
                            "Fields": [
                                { "Id": 1, "Name": "Document Type", "TypeDefinition": { "Name": "Enum" } },
                                { "Id": 3, "Name": "Document No.", "TypeDefinition": { "Name": "Code", "Length": 20 } }
                            ]
                        }
                    ],
                    "Pages": [
                        { "Id": 42, "Name": "Sales Order", "Properties": [{ "Name": "SourceTable", "Value": "#appid#Sales Header" }] }
                    ]
                },
                {
                    "Name": "Microsoft.Sales.Customer",
                    "Tables": [
                        { "Id": 18, "Name": "Customer", "Fields": [
                            { "Id": 1, "Name": "No.", "TypeDefinition": { "Name": "Code", "Length": 20 } }
                        ]}
                    ]
                }
            ]
        }"##;

        let objects = parse_symbols_from_json(json.as_bytes(), "Base App v26.0").unwrap();

        // All objects from all namespace levels must be present
        let names: Vec<&str> = objects.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"Language"), "Missing Language from Microsoft.Foundation");
        assert!(names.contains(&"Country/Region"), "Missing Country/Region from nested Microsoft.Foundation.Address");
        assert!(names.contains(&"Sales Header"), "Missing Sales Header from Microsoft.Sales.Document");
        assert!(names.contains(&"Sales Line"), "Missing Sales Line from Microsoft.Sales.Document");
        assert!(names.contains(&"Sales Order"), "Missing Sales Order page");
        assert!(names.contains(&"Customer"), "Missing Customer from Microsoft.Sales.Customer");

        assert_eq!(objects.len(), 6);

        let sales_header = objects.iter().find(|o| o.name == "Sales Header").unwrap();
        assert_eq!(sales_header.id, 36);
        assert_eq!(sales_header.object_type, ALObjectType::Table);
        assert_eq!(sales_header.namespace.as_deref(), Some("Microsoft.Sales.Document"));
        assert_eq!(sales_header.fields.len(), 3);
        assert_eq!(sales_header.keys.len(), 1);
        assert_eq!(sales_header.procedures.len(), 2);
        assert_eq!(sales_header.procedures[1].parameters.len(), 1);

        let country = objects.iter().find(|o| o.name == "Country/Region").unwrap();
        assert_eq!(country.namespace.as_deref(), Some("Microsoft.Foundation.Address"));
    }

    #[test]
    fn test_parse_enums_key() {
        let json = r##"{
            "Enums": [
                { "Id": 5, "Name": "Sales Document Type",
                  "Values": [
                    { "Ordinal": 0, "Name": "Quote" },
                    { "Ordinal": 1, "Name": "Order" },
                    { "Ordinal": 2, "Name": "Invoice" },
                    { "Ordinal": 3, "Name": "Credit Memo" },
                    { "Ordinal": 4, "Name": "Blanket Order" },
                    { "Ordinal": 5, "Name": "Return Order" }
                  ]
                }
            ]
        }"##;

        let objects = parse_symbols_from_json(json.as_bytes(), "Test").unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].name, "Sales Document Type");
        assert_eq!(objects[0].object_type, ALObjectType::Enum);
        assert_eq!(objects[0].enum_values.len(), 6);
        assert_eq!(objects[0].enum_values[1].name, "Order");
    }

    #[test]
    fn test_parse_query_elements() {
        let json = r##"{
            "Queries": [
                {
                    "Id": 105,
                    "Name": "Customer Sales Quantities",
                    "Elements": [
                        {
                            "Name": "Cust",
                            "RelatedTable": "#appid#Customer",
                            "Columns": [
                                { "Name": "Customer_No", "SourceColumn": "No." },
                                { "Name": "Customer_Name", "SourceColumn": "Name" }
                            ],
                            "Elements": [
                                {
                                    "Name": "SalesLine",
                                    "RelatedTable": "#appid#Sales Line",
                                    "Columns": [
                                        { "Name": "Quantity", "SourceColumn": "Quantity" }
                                    ]
                                }
                            ]
                        }
                    ]
                }
            ]
        }"##;

        let objects = parse_symbols_from_json(json.as_bytes(), "Test").unwrap();
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].name, "Customer Sales Quantities");
        assert_eq!(objects[0].object_type, ALObjectType::Query);
        assert_eq!(objects[0].data_items.len(), 1);
        assert_eq!(objects[0].data_items[0].name, "Cust");
        assert_eq!(objects[0].data_items[0].table_name.as_deref(), Some("Customer"));
        assert_eq!(objects[0].data_items[0].columns.len(), 2);
        assert_eq!(objects[0].data_items[0].columns[0].source_expression.as_deref(), Some("No."));
        assert_eq!(objects[0].data_items[0].children.len(), 1);
        assert_eq!(objects[0].data_items[0].children[0].table_name.as_deref(), Some("Sales Line"));
    }

    #[test]
    fn test_large_symbol_object_counts() {
        let make_table = |i: usize| -> serde_json::Value {
            serde_json::json!({
                "Id": i, "Name": format!("Table{}", i),
                "Fields": [{"Id": 1, "Name": "PK", "TypeDefinition": {"Name": "Code"}}],
                "Methods": [{"Name": "Init", "Parameters": []}]
            })
        };
        let make_page = |i: usize| -> serde_json::Value {
            serde_json::json!({
                "Id": i, "Name": format!("Page{}", i),
                "Properties": [{"Name": "SourceTable", "Value": format!("Table{}", i)}]
            })
        };
        let make_cu = |i: usize| -> serde_json::Value {
            serde_json::json!({
                "Id": i, "Name": format!("Codeunit{}", i),
                "Methods": [{"Name": "Run", "Parameters": []}]
            })
        };

        let tables: Vec<_> = (1..=200).map(make_table).collect();
        let pages: Vec<_> = (1..=150).map(make_page).collect();
        let codeunits: Vec<_> = (1..=100).map(make_cu).collect();

        let root = serde_json::json!({
            "Namespaces": [
                {"Name": "Test.Namespace.One", "Tables": tables, "Pages": pages},
                {"Name": "Test.Namespace.Two", "Codeunits": codeunits}
            ]
        });

        let json = serde_json::to_vec(&root).unwrap();
        let objects = parse_symbols_from_json(&json, "LargeApp v1.0").unwrap();
        assert_eq!(objects.len(), 450, "Expected 200 tables + 150 pages + 100 codeunits");

        let table_count = objects.iter().filter(|o| o.object_type == ALObjectType::Table).count();
        assert_eq!(table_count, 200);

        let page_count = objects.iter().filter(|o| o.object_type == ALObjectType::Page).count();
        assert_eq!(page_count, 150);

        let cu_count = objects.iter().filter(|o| o.object_type == ALObjectType::Codeunit).count();
        assert_eq!(cu_count, 100);

        let table1 = objects.iter().find(|o| o.name == "Table1").unwrap();
        assert_eq!(table1.fields.len(), 1);
        assert_eq!(table1.procedures.len(), 1);
    }

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
