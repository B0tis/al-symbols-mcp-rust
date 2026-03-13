use crate::al_scanner;
use crate::database::SymbolDatabase;
use crate::package_manager::PackageManager;
use crate::types::*;
use rmcp::model::*;
use rmcp::{Error as McpError, ServerHandler, tool};
#[allow(unused_imports)]
use rmcp::model::Implementation;
use serde::Deserialize;
use schemars::JsonSchema;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct AlMcpServer {
    package_manager: Arc<PackageManager>,
}

impl AlMcpServer {
    pub fn new() -> Self {
        let db = SymbolDatabase::new();
        let pm = Arc::new(PackageManager::new(db));
        Self {
            package_manager: pm,
        }
    }

    fn ensure_loaded(&self) {
        if !self.package_manager.is_loaded() {
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".into());
            eprintln!("Auto-discovering packages from {}...", cwd);

            let cli_status = self.package_manager.al_cli_status();
            if cli_status.available {
                eprintln!("AL CLI: {} ({})", cli_status.path, cli_status.version.as_deref().unwrap_or("unknown"));
            } else {
                eprintln!("AL CLI: not found — runtime packages will be skipped. Use al_cli_status tool to install.");
            }

            let start = std::time::Instant::now();
            match self.package_manager.auto_discover_and_load(&cwd) {
                Ok(result) => {
                    eprintln!(
                        "Loaded {} packages ({} objects) in {:.1}s",
                        result.packages_loaded,
                        result.objects_loaded,
                        start.elapsed().as_secs_f64()
                    );
                    for err in &result.errors {
                        eprintln!("  Warning: {}", err);
                    }
                }
                Err(e) => {
                    eprintln!("Auto-discovery failed: {}", e);
                }
            }
        }
    }

    fn load_error_hint(&self) -> Option<String> {
        let errors = self.package_manager.load_errors();
        if errors.is_empty() {
            return None;
        }
        let cli_status = self.package_manager.al_cli_status();
        let cli_hint = if !cli_status.available {
            " Some packages may be runtime-only and require the AL CLI to extract symbols. \
             Use al_cli_status with action 'install' to set up AL CLI, then reload with al_packages action 'load'."
        } else {
            ""
        };
        Some(format!(
            "WARNING: {} package(s) failed to load. Use al_packages with action 'list' to see details.{} Errors: {}",
            errors.len(),
            cli_hint,
            errors.join("; ")
        ))
    }

    fn db(&self) -> &SymbolDatabase {
        self.package_manager.database()
    }

    fn text_result(content: impl Into<String>) -> CallToolResult {
        CallToolResult::success(vec![Content::text(content.into())])
    }

    fn json_result(value: &impl serde::Serialize) -> CallToolResult {
        match serde_json::to_string_pretty(value) {
            Ok(json) => Self::text_result(json),
            Err(e) => CallToolResult::error(vec![Content::text(format!("Serialization error: {}", e))]),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchObjectsParams {
    /// Search pattern (wildcards supported: * and ?)
    #[serde(default)]
    pattern: Option<String>,
    /// Filter by object type (Table, Page, Codeunit, Report, Enum, etc.)
    #[serde(default)]
    object_type: Option<String>,
    /// Maximum results to return (default: 50)
    #[serde(default = "default_limit")]
    limit: usize,
    /// Offset for pagination (default: 0)
    #[serde(default)]
    offset: usize,
    /// Return summarized results for token efficiency
    #[serde(default)]
    summary_mode: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetObjectDefinitionParams {
    /// Object type (Table, Page, Codeunit, etc.)
    object_type: String,
    /// Object ID (numeric)
    #[serde(default)]
    id: Option<i64>,
    /// Object name (alternative to ID)
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FindReferencesParams {
    /// Name of the object to find references for
    object_name: String,
    /// Optional object type filter
    #[serde(default)]
    object_type: Option<String>,
    /// Optional field name for field-level references
    #[serde(default)]
    field_name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchMembersParams {
    /// Object name to search within (optional - searches all if omitted)
    #[serde(default)]
    object_name: Option<String>,
    /// Object type filter
    #[serde(default)]
    object_type: Option<String>,
    /// Member type: procedure, field, control, dataitem, or all
    #[serde(default)]
    member_type: Option<String>,
    /// Search pattern for member names
    #[serde(default)]
    pattern: Option<String>,
    /// Maximum results (default: 50)
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetObjectSummaryParams {
    /// Object type (Table, Page, Codeunit, etc.)
    object_type: String,
    /// Object name
    name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct PackagesParams {
    /// Action: "load", "list", or "stats"
    action: String,
    /// Directory path (required for "load" action)
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GetFreeIdParams {
    /// Filter by object type (table, page, codeunit, report, enum, etc.). When omitted, returns the next free ID for every object type found in the workspace.
    #[serde(default)]
    object_type: Option<String>,
    /// How many free IDs to return per type (default: 1)
    #[serde(default = "default_count")]
    count: usize,
    /// Path to app.json. Auto-detected from the workspace root — only needed if auto-detection fails.
    #[serde(default)]
    app_json_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AlCliStatusParams {
    /// Action: "status" to check AL CLI availability, "install" to attempt auto-installation
    #[serde(default = "default_cli_action")]
    action: String,
}

fn default_cli_action() -> String {
    "status".into()
}

fn default_count() -> usize {
    1
}

const AL_OBJECT_TYPES: &[&str] = &[
    "table",
    "tableextension",
    "page",
    "pageextension",
    "codeunit",
    "report",
    "reportextension",
    "xmlport",
    "enum",
    "enumextension",
    "query",
    "permissionset",
    "permissionsetextension",
    "controladdin",
    "interface",
];

fn default_limit() -> usize {
    50
}

#[tool(tool_box)]
impl AlMcpServer {
    #[tool(
        name = "al_search_objects",
        description = "Search AL objects in YOUR WORKSPACE (.app packages). Analyzes compiled AL code structure. Use summaryMode:true & limit for token efficiency. Supports wildcard patterns (* and ?)."
    )]
    fn search_objects(
        &self,
        #[tool(aggr)] params: SearchObjectsParams,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_loaded();

        let (results, total) = self.db().search_objects(
            params.pattern.as_deref(),
            params.object_type.as_deref(),
            params.limit,
            params.offset,
        );

        let load_warning = self.load_error_hint();

        if params.summary_mode {
            let summary: Vec<serde_json::Value> = results
                .iter()
                .map(|obj| {
                    serde_json::json!({
                        "id": obj.id,
                        "name": obj.name,
                        "type": obj.object_type.to_string(),
                        "package": obj.package_name,
                        "namespace": obj.namespace,
                        "fieldCount": obj.fields.len(),
                        "procedureCount": obj.procedures.len(),
                    })
                })
                .collect();
            Ok(Self::json_result(&serde_json::json!({
                "total": total,
                "returned": summary.len(),
                "offset": params.offset,
                "results": summary,
                "loadWarning": load_warning,
            })))
        } else {
            Ok(Self::json_result(&serde_json::json!({
                "total": total,
                "returned": results.len(),
                "offset": params.offset,
                "results": results,
                "loadWarning": load_warning,
            })))
        }
    }

    #[tool(
        name = "al_get_object_definition",
        description = "Get detailed object definition by ID or name, including fields, procedures, keys, and properties."
    )]
    fn get_object_definition(
        &self,
        #[tool(aggr)] params: GetObjectDefinitionParams,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_loaded();

        let obj_type = ALObjectType::from_str_loose(&params.object_type).to_string();

        let result = if let Some(id) = params.id {
            self.db().get_object_by_type_id(&obj_type, id)
        } else if let Some(ref name) = params.name {
            self.db().get_object_by_type_name(&obj_type, name)
        } else {
            None
        };

        match result {
            Some(obj) => {
                let extensions = self.db().get_extensions_for(&obj.name);
                Ok(Self::json_result(&serde_json::json!({
                    "object": obj,
                    "extensions": extensions.iter().map(|e| {
                        serde_json::json!({
                            "name": e.name,
                            "type": e.object_type.to_string(),
                            "id": e.id,
                            "package": e.package_name,
                        })
                    }).collect::<Vec<_>>(),
                })))
            }
            None => Ok(Self::json_result(&serde_json::json!({
                "error": "Object not found",
                "objectType": obj_type,
                "id": params.id,
                "name": params.name,
            }))),
        }
    }

    #[tool(
        name = "al_find_references",
        description = "Find all references to an object or field. Discovers extensions, variable usages, parameters, table relations, and more."
    )]
    fn find_references(
        &self,
        #[tool(aggr)] params: FindReferencesParams,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_loaded();

        let mut result = serde_json::json!({
            "objectName": params.object_name,
        });

        let refs = self.db().find_references(
            &params.object_name,
            params.object_type.as_deref(),
        );

        let extensions = self.db().get_extensions_for(&params.object_name);

        if let Some(ref field_name) = params.field_name {
            let field_refs = self
                .db()
                .find_field_references(&params.object_name, field_name);
            result["fieldReferences"] = serde_json::to_value(&field_refs).unwrap_or_default();
        }

        result["references"] = serde_json::to_value(&refs).unwrap_or_default();
        result["extensions"] = serde_json::json!(extensions.iter().map(|e| {
            serde_json::json!({
                "name": e.name,
                "type": e.object_type.to_string(),
                "id": e.id,
                "package": e.package_name,
            })
        }).collect::<Vec<serde_json::Value>>());
        result["totalReferences"] = serde_json::json!(refs.len());

        Ok(Self::json_result(&result))
    }

    #[tool(
        name = "al_search_object_members",
        description = "Search for procedures, fields, controls, or data items within objects. Unified member search across the symbol database."
    )]
    fn search_object_members(
        &self,
        #[tool(aggr)] params: SearchMembersParams,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_loaded();

        let results = self.db().search_members(
            params.object_name.as_deref(),
            params.object_type.as_deref(),
            params.member_type.as_deref(),
            params.pattern.as_deref(),
            params.limit,
        );

        Ok(Self::json_result(&serde_json::json!({
            "total": results.len(),
            "results": results,
        })))
    }

    #[tool(
        name = "al_get_object_summary",
        description = "Token-efficient categorized overview of an AL object. Groups procedures by category, shows field/key/control counts."
    )]
    fn get_object_summary(
        &self,
        #[tool(aggr)] params: GetObjectSummaryParams,
    ) -> Result<CallToolResult, McpError> {
        self.ensure_loaded();

        let obj_type = ALObjectType::from_str_loose(&params.object_type).to_string();
        let result = self.db().get_object_by_type_name(&obj_type, &params.name);

        match result {
            Some(obj) => {
                let mut summary = serde_json::json!({
                    "id": obj.id,
                    "name": obj.name,
                    "type": obj.object_type.to_string(),
                    "namespace": obj.namespace,
                    "package": obj.package_name,
                });

                if let Some(ref target) = obj.target_object {
                    summary["targetObject"] = serde_json::json!(target);
                }
                if let Some(ref st) = obj.source_table {
                    summary["sourceTable"] = serde_json::json!(st);
                }

                if !obj.fields.is_empty() {
                    summary["fieldCount"] = serde_json::json!(obj.fields.len());
                    summary["fields"] = serde_json::json!(
                        obj.fields.iter().map(|f| {
                            serde_json::json!({
                                "id": f.id,
                                "name": f.name,
                                "type": f.type_definition.name,
                            })
                        }).collect::<Vec<_>>()
                    );
                }

                if !obj.keys.is_empty() {
                    summary["keyCount"] = serde_json::json!(obj.keys.len());
                    summary["keys"] = serde_json::json!(
                        obj.keys.iter().map(|k| {
                            serde_json::json!({
                                "name": k.name,
                                "fields": k.field_names,
                            })
                        }).collect::<Vec<_>>()
                    );
                }

                if !obj.procedures.is_empty() {
                    let mut categorized: std::collections::HashMap<String, Vec<serde_json::Value>> =
                        std::collections::HashMap::new();

                    for proc in &obj.procedures {
                        let category = categorize_procedure(&proc.name, &proc.properties);
                        let entry = serde_json::json!({
                            "name": proc.name,
                            "params": proc.parameters.iter().map(|p| {
                                format!("{}{}: {}", if p.is_var { "var " } else { "" }, p.name, p.type_definition.name)
                            }).collect::<Vec<_>>(),
                            "returnType": proc.return_type.as_ref().map(|r| &r.name),
                        });
                        categorized.entry(category).or_default().push(entry);
                    }

                    summary["procedureCount"] = serde_json::json!(obj.procedures.len());
                    summary["procedures"] = serde_json::to_value(&categorized).unwrap_or_default();
                }

                if !obj.enum_values.is_empty() {
                    summary["valueCount"] = serde_json::json!(obj.enum_values.len());
                    summary["values"] = serde_json::json!(
                        obj.enum_values.iter().map(|v| {
                            serde_json::json!({
                                "ordinal": v.ordinal,
                                "name": v.name,
                            })
                        }).collect::<Vec<_>>()
                    );
                }

                if !obj.controls.is_empty() {
                    summary["controlCount"] = serde_json::json!(count_controls(&obj.controls));
                }

                if !obj.data_items.is_empty() {
                    summary["dataItemCount"] = serde_json::json!(count_data_items(&obj.data_items));
                }

                let extensions = self.db().get_extensions_for(&obj.name);
                if !extensions.is_empty() {
                    summary["extensionCount"] = serde_json::json!(extensions.len());
                    summary["extensions"] = serde_json::json!(
                        extensions.iter().map(|e| {
                            serde_json::json!({
                                "name": e.name,
                                "type": e.object_type.to_string(),
                                "package": e.package_name,
                            })
                        }).collect::<Vec<_>>()
                    );
                }

                Ok(Self::json_result(&summary))
            }
            None => Ok(Self::json_result(&serde_json::json!({
                "error": "Object not found",
                "objectType": obj_type,
                "name": params.name,
            }))),
        }
    }

    #[tool(
        name = "al_get_free_id",
        description = "Get the next free object ID(s) for your AL app. Automatically finds app.json in the workspace root, reads idRanges, and scans local .al source files to find unused IDs. When no object_type is given, returns the next free ID for EVERY object type. No parameters required for typical use."
    )]
    fn get_free_id(
        &self,
        #[tool(aggr)] params: GetFreeIdParams,
    ) -> Result<CallToolResult, McpError> {
        let app_json_path = match params.app_json_path {
            Some(ref p) => PathBuf::from(p),
            None => find_app_json()?,
        };

        let app_dir = app_json_path
            .parent()
            .ok_or_else(|| McpError::internal_error("Cannot determine app directory from app.json path", None))?;

        let ranges = parse_id_ranges(&app_json_path)?;

        if ranges.is_empty() {
            return Ok(Self::json_result(&serde_json::json!({
                "error": "No idRanges found in app.json",
                "appJsonPath": app_json_path.to_string_lossy(),
            })));
        }

        let count = params.count.max(1).min(100);

        let types_to_query: Vec<&str> = match params.object_type {
            Some(ref t) => vec![leak_str(t)],
            None => AL_OBJECT_TYPES.to_vec(),
        };

        // Scan all source objects once (no type filter) for the full picture
        let all_source_objects = al_scanner::scan_al_sources(app_dir, None);

        let mut per_type: Vec<serde_json::Value> = Vec::new();

        for obj_type in &types_to_query {
            let used: BTreeSet<i64> = all_source_objects
                .iter()
                .filter(|o| o.object_type == *obj_type)
                .map(|o| o.id)
                .collect();

            let mut free_ids: Vec<i64> = Vec::with_capacity(count);
            'outer: for range in &ranges {
                for id in range.from..=range.to {
                    if !used.contains(&id) {
                        free_ids.push(id);
                        if free_ids.len() >= count {
                            break 'outer;
                        }
                    }
                }
            }

            let used_in_ranges: usize = used
                .iter()
                .filter(|&&id| ranges.iter().any(|r| id >= r.from && id <= r.to))
                .count();

            per_type.push(serde_json::json!({
                "objectType": obj_type,
                "nextFreeId": free_ids.first(),
                "freeIds": free_ids,
                "usedCount": used_in_ranges,
            }));
        }

        let total_capacity: i64 = ranges.iter().map(|r| r.to - r.from + 1).sum();

        let used_objects: Vec<serde_json::Value> = all_source_objects
            .iter()
            .filter(|o| ranges.iter().any(|r| o.id >= r.from && o.id <= r.to))
            .map(|o| {
                serde_json::json!({
                    "objectType": o.object_type,
                    "id": o.id,
                    "name": o.name,
                    "file": o.file,
                })
            })
            .collect();

        Ok(Self::json_result(&serde_json::json!({
            "perObjectType": per_type,
            "idRanges": ranges.iter().map(|r| serde_json::json!({ "from": r.from, "to": r.to })).collect::<Vec<_>>(),
            "totalCapacity": total_capacity,
            "totalUsed": used_objects.len(),
            "usedObjects": used_objects,
            "appDir": app_dir.to_string_lossy(),
            "appJsonPath": app_json_path.to_string_lossy(),
        })))
    }

    #[tool(
        name = "al_cli_status",
        description = "Check AL CLI availability or attempt auto-installation. The AL CLI (from Microsoft.Dynamics.BusinessCentral.Development.Tools) is needed to load runtime packages that don't contain SymbolReference.json — this includes the Base Application which contains core tables like 'Sales Header', 'Sales Line', 'Purchase Header', etc. Actions: 'status' (check availability) or 'install' (auto-install via dotnet)."
    )]
    fn al_cli_status(
        &self,
        #[tool(aggr)] params: AlCliStatusParams,
    ) -> Result<CallToolResult, McpError> {
        match params.action.as_str() {
            "status" => {
                let status = self.package_manager.al_cli_status();
                Ok(Self::json_result(&serde_json::json!({
                    "available": status.available,
                    "path": status.path,
                    "version": status.version,
                    "message": status.message,
                    "installInstructions": if status.available { None } else {
                        Some(crate::al_cli::AlCli::install_instructions())
                    },
                })))
            }
            "install" => {
                match self.package_manager.try_install_al_cli() {
                    Ok(msg) => {
                        let status = self.package_manager.al_cli_status();
                        Ok(Self::json_result(&serde_json::json!({
                            "success": true,
                            "message": msg,
                            "available": status.available,
                            "version": status.version,
                            "hint": "Run al_packages with action 'load' to reload packages with AL CLI support.",
                        })))
                    }
                    Err(e) => {
                        Ok(Self::json_result(&serde_json::json!({
                            "success": false,
                            "error": format!("{}", e),
                            "installInstructions": crate::al_cli::AlCli::install_instructions(),
                        })))
                    }
                }
            }
            other => Ok(Self::json_result(&serde_json::json!({
                "error": format!("Unknown action: {}. Use 'status' or 'install'.", other),
            }))),
        }
    }

    #[tool(
        name = "al_packages",
        description = "Package management: load packages from a directory, list loaded packages, or get package statistics. Actions: 'load', 'list', 'stats'."
    )]
    fn packages(
        &self,
        #[tool(aggr)] params: PackagesParams,
    ) -> Result<CallToolResult, McpError> {
        match params.action.as_str() {
            "load" => {
                let path = params.path.unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| ".".into())
                });

                self.db().clear();

                match self.package_manager.auto_discover_and_load(&path) {
                    Ok(result) => Ok(Self::json_result(&result)),
                    Err(e) => Ok(Self::json_result(&serde_json::json!({
                        "error": format!("Failed to load packages: {}", e),
                    }))),
                }
            }
            "list" => {
                self.ensure_loaded();
                let packages = self.package_manager.loaded_packages();
                let errors = self.package_manager.load_errors();
                Ok(Self::json_result(&serde_json::json!({
                    "totalPackages": packages.len(),
                    "totalObjects": self.db().object_count(),
                    "packages": packages.iter().map(|p| {
                        serde_json::json!({
                            "name": p.name,
                            "version": p.version,
                            "publisher": p.publisher,
                            "id": p.id,
                            "dependencyCount": p.dependencies.len(),
                        })
                    }).collect::<Vec<_>>(),
                    "failedPackages": errors.len(),
                    "loadErrors": errors,
                })))
            }
            "stats" => {
                self.ensure_loaded();
                let stats = self.db().package_stats();
                Ok(Self::json_result(&serde_json::json!({
                    "totalPackages": stats.len(),
                    "totalObjects": self.db().object_count(),
                    "packages": stats,
                })))
            }
            other => Ok(Self::json_result(&serde_json::json!({
                "error": format!("Unknown action: {}. Use 'load', 'list', or 'stats'.", other),
            }))),
        }
    }
}

#[tool(tool_box)]
impl ServerHandler for AlMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "AL Symbol MCP Server - Analyzes Business Central AL packages (.app files) \
                 for dependency analysis, object search, and symbol reference resolution. \
                 Packages are auto-loaded from the workspace on first tool call."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "al-symbols-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct IdRange {
    from: i64,
    to: i64,
}

fn find_app_json() -> Result<PathBuf, McpError> {
    let cwd = std::env::current_dir().map_err(|e| {
        McpError::internal_error(format!("Cannot determine working directory: {}", e), None)
    })?;

    // 1. Check root directory first (most common location)
    let root_app_json = cwd.join("app.json");
    if root_app_json.is_file() {
        return Ok(root_app_json);
    }

    // 2. Walk up to 3 levels deep, but skip .alpackages / .snapshots / node_modules
    let exclude: std::collections::HashSet<&str> =
        [".alpackages", ".snapshots", "node_modules", ".git", "target"]
            .iter()
            .copied()
            .collect();

    for entry in walkdir::WalkDir::new(&cwd)
        .max_depth(3)
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
        if entry.file_type().is_file() && entry.file_name().eq_ignore_ascii_case("app.json") {
            return Ok(entry.into_path());
        }
    }

    Err(McpError::internal_error(
        format!(
            "No app.json found under {}. Provide the path explicitly via the app_json_path parameter.",
            cwd.display()
        ),
        None,
    ))
}

fn parse_id_ranges(app_json_path: &Path) -> Result<Vec<IdRange>, McpError> {
    let content = std::fs::read_to_string(app_json_path).map_err(|e| {
        McpError::internal_error(
            format!("Cannot read {}: {}", app_json_path.display(), e),
            None,
        )
    })?;

    let val: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        McpError::internal_error(
            format!("Invalid JSON in {}: {}", app_json_path.display(), e),
            None,
        )
    })?;

    let ranges_val = val.get("idRanges").or_else(|| val.get("idRange"));

    let ranges = match ranges_val {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|r| {
                let from = r.get("from").and_then(|v| v.as_i64())?;
                let to = r.get("to").and_then(|v| v.as_i64())?;
                Some(IdRange { from, to })
            })
            .collect(),
        Some(serde_json::Value::Object(obj)) => {
            let from = obj.get("from").and_then(|v| v.as_i64());
            let to = obj.get("to").and_then(|v| v.as_i64());
            match (from, to) {
                (Some(f), Some(t)) => vec![IdRange { from: f, to: t }],
                _ => vec![],
            }
        }
        _ => vec![],
    };

    Ok(ranges)
}

fn leak_str(s: &str) -> &'static str {
    let normalized = crate::al_scanner::normalize_type(s);
    Box::leak(normalized.into_boxed_str())
}

fn categorize_procedure(name: &str, properties: &[ALProperty]) -> String {
    let name_lower = name.to_lowercase();

    for prop in properties {
        if prop.name == "EventSubscriber" || prop.name == "IntegrationEvent" || prop.name == "BusinessEvent" {
            return "Events".into();
        }
    }

    if name_lower.starts_with("on") && name_lower.len() > 2 {
        return "Triggers/Events".into();
    }
    if name_lower.starts_with("get") || name_lower.starts_with("is") || name_lower.starts_with("has") {
        return "Getters".into();
    }
    if name_lower.starts_with("set") || name_lower.starts_with("update") {
        return "Setters".into();
    }
    if name_lower.starts_with("validate") || name_lower.starts_with("check") || name_lower.starts_with("test") {
        return "Validation".into();
    }
    if name_lower.starts_with("insert")
        || name_lower.starts_with("modify")
        || name_lower.starts_with("delete")
        || name_lower.starts_with("create")
    {
        return "CRUD".into();
    }
    if name_lower.starts_with("calc") || name_lower.starts_with("compute") {
        return "Calculations".into();
    }
    if name_lower.starts_with("init") || name_lower.starts_with("setup") {
        return "Initialization".into();
    }

    "Other".into()
}

fn count_controls(controls: &[ALControl]) -> usize {
    let mut count = controls.len();
    for ctrl in controls {
        count += count_controls(&ctrl.children);
    }
    count
}

fn count_data_items(items: &[ALDataItem]) -> usize {
    let mut count = items.len();
    for item in items {
        count += count_data_items(&item.children);
    }
    count
}
