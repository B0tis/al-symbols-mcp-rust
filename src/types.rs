use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ALObjectType {
    Table,
    TableExtension,
    Page,
    PageExtension,
    PageCustomization,
    Codeunit,
    Report,
    ReportExtension,
    Query,
    XmlPort,
    Enum,
    EnumExtension,
    Interface,
    PermissionSet,
    PermissionSetExtension,
    ControlAddIn,
    Profile,
    Entitlement,
    Unknown(String),
}

impl fmt::Display for ALObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ALObjectType::Table => write!(f, "Table"),
            ALObjectType::TableExtension => write!(f, "TableExtension"),
            ALObjectType::Page => write!(f, "Page"),
            ALObjectType::PageExtension => write!(f, "PageExtension"),
            ALObjectType::PageCustomization => write!(f, "PageCustomization"),
            ALObjectType::Codeunit => write!(f, "Codeunit"),
            ALObjectType::Report => write!(f, "Report"),
            ALObjectType::ReportExtension => write!(f, "ReportExtension"),
            ALObjectType::Query => write!(f, "Query"),
            ALObjectType::XmlPort => write!(f, "XmlPort"),
            ALObjectType::Enum => write!(f, "Enum"),
            ALObjectType::EnumExtension => write!(f, "EnumExtension"),
            ALObjectType::Interface => write!(f, "Interface"),
            ALObjectType::PermissionSet => write!(f, "PermissionSet"),
            ALObjectType::PermissionSetExtension => write!(f, "PermissionSetExtension"),
            ALObjectType::ControlAddIn => write!(f, "ControlAddIn"),
            ALObjectType::Profile => write!(f, "Profile"),
            ALObjectType::Entitlement => write!(f, "Entitlement"),
            ALObjectType::Unknown(s) => write!(f, "{}", s),
        }
    }
}

impl ALObjectType {
    pub fn from_plural(s: &str) -> Self {
        match s {
            "Tables" => ALObjectType::Table,
            "TableExtensions" => ALObjectType::TableExtension,
            "Pages" => ALObjectType::Page,
            "PageExtensions" => ALObjectType::PageExtension,
            "PageCustomizations" => ALObjectType::PageCustomization,
            "Codeunits" => ALObjectType::Codeunit,
            "Reports" => ALObjectType::Report,
            "ReportExtensions" => ALObjectType::ReportExtension,
            "Queries" => ALObjectType::Query,
            "XmlPorts" => ALObjectType::XmlPort,
            "EnumTypes" | "Enums" => ALObjectType::Enum,
            "EnumExtensionTypes" => ALObjectType::EnumExtension,
            "Interfaces" => ALObjectType::Interface,
            "PermissionSets" => ALObjectType::PermissionSet,
            "PermissionSetExtensions" => ALObjectType::PermissionSetExtension,
            "ControlAddIns" => ALObjectType::ControlAddIn,
            "Profiles" => ALObjectType::Profile,
            "Entitlements" => ALObjectType::Entitlement,
            other => ALObjectType::Unknown(other.to_string()),
        }
    }

    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "table" => ALObjectType::Table,
            "tableextension" | "table extension" => ALObjectType::TableExtension,
            "page" => ALObjectType::Page,
            "pageextension" | "page extension" => ALObjectType::PageExtension,
            "pagecustomization" | "page customization" => ALObjectType::PageCustomization,
            "codeunit" => ALObjectType::Codeunit,
            "report" => ALObjectType::Report,
            "reportextension" | "report extension" => ALObjectType::ReportExtension,
            "query" => ALObjectType::Query,
            "xmlport" => ALObjectType::XmlPort,
            "enum" => ALObjectType::Enum,
            "enumextension" | "enum extension" | "enumextensiontype" => ALObjectType::EnumExtension,
            "interface" => ALObjectType::Interface,
            "permissionset" | "permission set" => ALObjectType::PermissionSet,
            "permissionsetextension" | "permission set extension" => {
                ALObjectType::PermissionSetExtension
            }
            "controladdin" | "control addin" | "control add-in" => ALObjectType::ControlAddIn,
            "profile" => ALObjectType::Profile,
            "entitlement" => ALObjectType::Entitlement,
            other => ALObjectType::Unknown(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALTypeDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<Box<ALTypeDefinition>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALProperty {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALField {
    pub id: i64,
    pub name: String,
    pub type_definition: ALTypeDefinition,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<ALProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALParameter {
    pub name: String,
    pub type_definition: ALTypeDefinition,
    #[serde(default)]
    pub is_var: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALProcedure {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_type: Option<ALTypeDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ALParameter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<ALProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALKey {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<ALProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALEnumValue {
    pub ordinal: i64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<ALProperty>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALControl {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<ALProperty>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ALControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALDataItem {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ALColumn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ALDataItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALColumn {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_expression: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALVariable {
    pub name: String,
    pub type_definition: ALTypeDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALObject {
    pub id: i64,
    pub name: String,
    pub object_type: ALObjectType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_object: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ALField>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<ALKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub procedures: Vec<ALProcedure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<ALProperty>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enum_values: Vec<ALEnumValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<ALControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_items: Vec<ALDataItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<ALVariable>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_table: Option<String>,
}

impl ALObject {
    pub fn type_id_key(&self) -> String {
        format!("{}:{}", self.object_type, self.id)
    }

    pub fn type_name_key(&self) -> String {
        format!("{}:{}", self.object_type, self.name.to_lowercase())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALPackageInfo {
    pub name: String,
    pub id: String,
    pub version: String,
    pub publisher: String,
    #[serde(default)]
    pub dependencies: Vec<ALPackageDependency>,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALPackageDependency {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALFieldReference {
    pub source_object_id: String,
    pub source_object_name: String,
    pub source_object_type: String,
    pub target_table_name: String,
    pub target_field_name: String,
    pub reference_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ALReference {
    pub source_name: String,
    pub source_type: String,
    pub target_name: String,
    pub target_type: String,
    pub reference_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}
