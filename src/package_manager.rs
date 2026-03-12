use crate::database::SymbolDatabase;
use crate::manifest::parse_manifest_from_app;
use crate::symbol_parser::parse_symbols_from_app;
use crate::types::ALPackageInfo;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

#[derive(Error, Debug)]
pub enum PackageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Manifest error: {0}")]
    Manifest(#[from] crate::manifest::ManifestError),
    #[error("Symbol parse error: {0}")]
    SymbolParse(#[from] crate::symbol_parser::SymbolParseError),
    #[error("Circular dependency detected involving: {0}")]
    CircularDependency(String),
}

pub struct PackageManager {
    database: SymbolDatabase,
    loaded_packages: parking_lot::Mutex<Vec<ALPackageInfo>>,
    loaded_dirs: parking_lot::Mutex<HashSet<String>>,
}

impl PackageManager {
    pub fn new(database: SymbolDatabase) -> Self {
        Self {
            database,
            loaded_packages: parking_lot::Mutex::new(Vec::new()),
            loaded_dirs: parking_lot::Mutex::new(HashSet::new()),
        }
    }

    pub fn database(&self) -> &SymbolDatabase {
        &self.database
    }

    pub fn loaded_packages(&self) -> Vec<ALPackageInfo> {
        self.loaded_packages.lock().clone()
    }

    pub fn is_loaded(&self) -> bool {
        !self.loaded_packages.lock().is_empty()
    }

    pub fn auto_discover_and_load(&self, root_path: &str) -> Result<LoadResult, PackageError> {
        let dirs = self.discover_package_directories(root_path)?;

        if dirs.is_empty() {
            return Ok(LoadResult {
                packages_loaded: 0,
                objects_loaded: 0,
                directories: vec![],
                errors: vec![format!("No .alpackages directories found under {}", root_path)],
            });
        }

        let mut all_app_files = Vec::new();
        for dir in &dirs {
            let apps = find_app_files(dir);
            all_app_files.extend(apps);
        }

        let unique_apps = filter_latest_versions(&all_app_files);

        info!(
            "Found {} unique packages (from {} total .app files) in {} directories",
            unique_apps.len(),
            all_app_files.len(),
            dirs.len()
        );

        let sorted = self.resolve_dependency_order(&unique_apps)?;

        let mut packages_loaded = 0;
        let mut objects_loaded = 0;
        let mut errors = Vec::new();

        for app_path in &sorted {
            match self.load_single_package(app_path) {
                Ok((pkg, count)) => {
                    info!("Loaded {} ({} objects)", pkg.name, count);
                    packages_loaded += 1;
                    objects_loaded += count;
                }
                Err(e) => {
                    let msg = format!("Failed to load {}: {}", app_path.display(), e);
                    warn!("{}", msg);
                    errors.push(msg);
                }
            }
        }

        {
            let mut loaded = self.loaded_dirs.lock();
            for dir in &dirs {
                loaded.insert(dir.clone());
            }
        }

        Ok(LoadResult {
            packages_loaded,
            objects_loaded,
            directories: dirs,
            errors,
        })
    }

    pub fn load_directory(&self, dir_path: &str) -> Result<LoadResult, PackageError> {
        let app_files = find_app_files(dir_path);
        let unique = filter_latest_versions(&app_files);
        let sorted = self.resolve_dependency_order(&unique)?;

        let mut packages_loaded = 0;
        let mut objects_loaded = 0;
        let mut errors = Vec::new();

        for app_path in &sorted {
            match self.load_single_package(app_path) {
                Ok((pkg, count)) => {
                    info!("Loaded {} ({} objects)", pkg.name, count);
                    packages_loaded += 1;
                    objects_loaded += count;
                }
                Err(e) => {
                    let msg = format!("Failed to load {}: {}", app_path.display(), e);
                    warn!("{}", msg);
                    errors.push(msg);
                }
            }
        }

        self.loaded_dirs.lock().insert(dir_path.to_string());

        Ok(LoadResult {
            packages_loaded,
            objects_loaded,
            directories: vec![dir_path.to_string()],
            errors,
        })
    }

    fn load_single_package(&self, app_path: &Path) -> Result<(ALPackageInfo, usize), PackageError> {
        let manifest = parse_manifest_from_app(app_path)?;
        let package_name = format!("{} v{}", manifest.name, manifest.version);
        let objects = parse_symbols_from_app(app_path, &package_name)?;
        let count = objects.len();
        self.database.add_objects(objects);
        self.loaded_packages.lock().push(manifest.clone());
        Ok((manifest, count))
    }

    fn discover_package_directories(&self, root_path: &str) -> Result<Vec<String>, PackageError> {
        let root = Path::new(root_path);
        if !root.exists() {
            return Ok(vec![]);
        }

        let mut dirs = Vec::new();

        // Look for .alpackages directories (max depth 3)
        for entry in WalkDir::new(root)
            .max_depth(3)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_dir() {
                let name = entry.file_name().to_string_lossy();
                if name == ".alpackages" {
                    dirs.push(entry.path().to_string_lossy().to_string());
                }
            }
        }

        // Also check for VS Code settings
        let vscode_settings = root.join(".vscode").join("settings.json");
        if vscode_settings.exists() {
            if let Ok(content) = std::fs::read_to_string(&vscode_settings) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(cache_path) = val
                        .get("al.packageCachePath")
                        .and_then(|v| v.as_str())
                    {
                        let resolved = if Path::new(cache_path).is_absolute() {
                            PathBuf::from(cache_path)
                        } else {
                            root.join(cache_path)
                        };
                        if resolved.exists() {
                            dirs.push(resolved.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        // Deduplicate
        dirs.sort();
        dirs.dedup();

        Ok(dirs)
    }

    fn resolve_dependency_order(&self, app_files: &[PathBuf]) -> Result<Vec<PathBuf>, PackageError> {
        let mut manifests: HashMap<String, (ALPackageInfo, PathBuf)> = HashMap::new();

        for app_path in app_files {
            match parse_manifest_from_app(app_path) {
                Ok(manifest) => {
                    let id = manifest.id.to_lowercase();
                    manifests.insert(id, (manifest, app_path.clone()));
                }
                Err(e) => {
                    debug!("Skipping {} (manifest error: {})", app_path.display(), e);
                }
            }
        }

        // Topological sort
        let mut sorted = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut visiting: HashSet<String> = HashSet::new();

        let ids: Vec<String> = manifests.keys().cloned().collect();

        fn visit(
            id: &str,
            manifests: &HashMap<String, (ALPackageInfo, PathBuf)>,
            visited: &mut HashSet<String>,
            visiting: &mut HashSet<String>,
            sorted: &mut Vec<PathBuf>,
        ) -> Result<(), PackageError> {
            if visited.contains(id) {
                return Ok(());
            }
            if visiting.contains(id) {
                return Err(PackageError::CircularDependency(id.to_string()));
            }
            visiting.insert(id.to_string());

            if let Some((manifest, _)) = manifests.get(id) {
                for dep in &manifest.dependencies {
                    let dep_id = dep.id.to_lowercase();
                    if manifests.contains_key(&dep_id) {
                        visit(&dep_id, manifests, visited, visiting, sorted)?;
                    }
                }
            }

            visiting.remove(id);
            visited.insert(id.to_string());

            if let Some((_, path)) = manifests.get(id) {
                sorted.push(path.clone());
            }

            Ok(())
        }

        for id in &ids {
            visit(id, &manifests, &mut visited, &mut visiting, &mut sorted)?;
        }

        Ok(sorted)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LoadResult {
    pub packages_loaded: usize,
    pub objects_loaded: usize,
    pub directories: Vec<String>,
    pub errors: Vec<String>,
}

fn find_app_files(dir: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let dir_path = Path::new(dir);
    if !dir_path.exists() {
        return files;
    }
    for entry in WalkDir::new(dir_path)
        .max_depth(2)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext.eq_ignore_ascii_case("app") {
                    files.push(entry.into_path());
                }
            }
        }
    }
    files
}

fn filter_latest_versions(app_files: &[PathBuf]) -> Vec<PathBuf> {
    let mut by_key: HashMap<String, (semver::Version, PathBuf)> = HashMap::new();

    for path in app_files {
        match parse_manifest_from_app(path) {
            Ok(manifest) => {
                let key = format!(
                    "{}_{}",
                    manifest.publisher.to_lowercase(),
                    manifest.name.to_lowercase()
                );
                let version = parse_version_lenient(&manifest.version);
                match by_key.get(&key) {
                    Some((existing, _)) if &version > existing => {
                        by_key.insert(key, (version, path.clone()));
                    }
                    None => {
                        by_key.insert(key, (version, path.clone()));
                    }
                    _ => {}
                }
            }
            Err(e) => {
                debug!("Skipping {} for version filter: {}", path.display(), e);
            }
        }
    }

    by_key.into_values().map(|(_, path)| path).collect()
}

fn parse_version_lenient(v: &str) -> semver::Version {
    let parts: Vec<&str> = v.split('.').collect();
    let major = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let pre = parts
        .get(3)
        .map(|s| {
            semver::Prerelease::new(s).unwrap_or(semver::Prerelease::EMPTY)
        })
        .unwrap_or(semver::Prerelease::EMPTY);
    semver::Version {
        major,
        minor,
        patch,
        pre,
        build: semver::BuildMetadata::EMPTY,
    }
}
