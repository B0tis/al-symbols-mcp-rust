use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Error, Debug)]
pub enum AlCliError {
    #[error("AL CLI not found. Install via: dotnet tool install --global {0} --prerelease")]
    NotFound(String),
    #[error("AL CLI command failed (exit {code}): {stderr}")]
    CommandFailed { code: i32, stderr: String },
    #[error("AL CLI process error: {0}")]
    ProcessError(#[from] std::io::Error),
    #[error("AL CLI timed out after {0} seconds")]
    Timeout(u64),
    #[error(".NET SDK not found — required for AL CLI installation")]
    DotnetNotFound,
    #[error("AL CLI installation failed: {0}")]
    InstallFailed(String),
}

#[derive(Debug, Clone)]
pub struct AlCliStatus {
    pub available: bool,
    pub path: String,
    pub version: Option<String>,
    pub message: String,
}

pub struct AlCli {
    al_command: String,
}

impl AlCli {
    /// Detect the AL CLI. Does NOT spawn any subprocess — just picks the
    /// command string. Call `probe()` to actually check availability.
    pub fn detect() -> Self {
        let al_command = std::env::var("AL_CLI_PATH").unwrap_or_else(|_| "AL".into());
        Self { al_command }
    }

    /// Probe whether the AL CLI is actually runnable and return a cached status
    /// snapshot. This spawns `AL --version` exactly once.
    pub fn probe(&self) -> AlCliStatus {
        if let Ok(version) = self.get_version() {
            return AlCliStatus {
                available: true,
                path: self.al_command.clone(),
                version: Some(version),
                message: format!("AL CLI available at {}", self.al_command),
            };
        }

        if let Some(found) = search_common_paths() {
            if let Ok(version) = run_version(&found) {
                return AlCliStatus {
                    available: true,
                    path: found,
                    version: Some(version),
                    message: "AL CLI found at alternate path".into(),
                };
            }
        }

        AlCliStatus {
            available: false,
            path: self.al_command.clone(),
            version: None,
            message: Self::install_instructions(),
        }
    }

    fn get_version(&self) -> Result<String, AlCliError> {
        run_version(&self.al_command)
    }

    /// Convert a runtime .app package into a symbol package containing
    /// SymbolReference.json.  Returns the path to the generated file.
    ///
    /// If `cache_dir` is `Some`, the result is stored there so future calls
    /// with the same input skip the conversion entirely.
    pub fn create_symbol_package(
        &self,
        app_path: &Path,
        cache_dir: Option<&Path>,
    ) -> Result<PathBuf, AlCliError> {
        if let Some(dir) = cache_dir {
            let cached = cached_symbol_path(dir, app_path);
            if cached.exists() {
                debug!("AL CLI cache hit: {}", cached.display());
                return Ok(cached);
            }
        }

        let out_path = match cache_dir {
            Some(dir) => {
                std::fs::create_dir_all(dir).ok();
                cached_symbol_path(dir, app_path)
            }
            None => std::env::temp_dir()
                .join(format!(
                    "al_sym_{}_{}",
                    std::process::id(),
                    app_path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                ))
                .with_extension("app"),
        };

        debug!(
            "AL CLI: CreateSymbolPackage {} -> {}",
            app_path.display(),
            out_path.display()
        );

        let output = Command::new(&self.al_command)
            .arg("CreateSymbolPackage")
            .arg(app_path)
            .arg(&out_path)
            .output()
            .map_err(AlCliError::ProcessError)?;

        if output.status.success() {
            if out_path.exists() {
                info!("AL CLI: Created symbol package at {}", out_path.display());
                Ok(out_path)
            } else {
                Err(AlCliError::CommandFailed {
                    code: 0,
                    stderr: "CreateSymbolPackage succeeded but output file not found".into(),
                })
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Err(AlCliError::CommandFailed {
                code: output.status.code().unwrap_or(-1),
                stderr: if stderr.is_empty() { stdout } else { stderr },
            })
        }
    }

    /// Try to auto-install the AL CLI using `dotnet tool install`.
    pub fn try_auto_install() -> Result<String, AlCliError> {
        if !is_dotnet_available() {
            return Err(AlCliError::DotnetNotFound);
        }

        let package_name = platform_package_name();
        info!(
            "Installing AL CLI: dotnet tool install --global {} --prerelease",
            package_name
        );

        let output = Command::new("dotnet")
            .args(["tool", "install", "--global", &package_name, "--prerelease"])
            .output()
            .map_err(AlCliError::ProcessError)?;

        if output.status.success() {
            let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
            info!("AL CLI installed: {}", msg);
            Ok(msg)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if stderr.contains("already installed") || stderr.contains("is already") {
                Ok("AL CLI is already installed".into())
            } else {
                Err(AlCliError::InstallFailed(stderr))
            }
        }
    }

    pub fn install_instructions() -> String {
        let pkg = platform_package_name();
        format!(
            "AL CLI not found. To install:\n\
             1. Install .NET SDK: https://dotnet.microsoft.com/download\n\
             2. Run: dotnet tool install --global {} --prerelease\n\
             3. Verify: AL --version\n\
             \n\
             Or set AL_CLI_PATH environment variable to the AL binary location.\n\
             \n\
             Without AL CLI, only packages that already contain SymbolReference.json \
             can be loaded. Runtime packages (like the Base Application) require \
             AL CLI for symbol extraction.",
            pkg
        )
    }

    /// Clean up a temporary (non-cached) symbol package file.
    pub fn cleanup_symbol_file(path: &Path) {
        if path.exists() {
            if let Err(e) = std::fs::remove_file(path) {
                warn!("Failed to clean up symbol file {}: {}", path.display(), e);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers (no subprocess unless explicitly called)
// ---------------------------------------------------------------------------

fn run_version(al_command: &str) -> Result<String, AlCliError> {
    let output = Command::new(al_command)
        .arg("--version")
        .output()
        .map_err(AlCliError::ProcessError)?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() || !stdout.is_empty() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Err(AlCliError::CommandFailed {
            code: output.status.code().unwrap_or(-1),
            stderr,
        })
    }
}

fn search_common_paths() -> Option<String> {
    let home = dirs_home();

    let candidates: Vec<PathBuf> = if cfg!(target_os = "windows") {
        vec![
            home.join(".dotnet").join("tools").join("AL.exe"),
            PathBuf::from(r"C:\Program Files\dotnet\tools\AL.exe"),
            PathBuf::from(r"C:\Program Files (x86)\dotnet\tools\AL.exe"),
        ]
    } else {
        vec![
            home.join(".dotnet").join("tools").join("AL"),
            PathBuf::from("/usr/local/share/dotnet/tools/AL"),
            PathBuf::from("/usr/share/dotnet/tools/AL"),
            PathBuf::from("/opt/dotnet/tools/AL"),
        ]
    };

    for c in candidates {
        if c.exists() {
            debug!("Found AL CLI candidate: {}", c.display());
            return Some(c.to_string_lossy().to_string());
        }
    }
    None
}

fn is_dotnet_available() -> bool {
    Command::new("dotnet")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn platform_package_name() -> String {
    if cfg!(target_os = "windows") {
        "Microsoft.Dynamics.BusinessCentral.Development.Tools".into()
    } else if cfg!(target_os = "macos") {
        "Microsoft.Dynamics.BusinessCentral.Development.Tools.Osx".into()
    } else {
        "Microsoft.Dynamics.BusinessCentral.Development.Tools.Linux".into()
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Deterministic cache path: `<cache_dir>/<stem>.symbols.app`
fn cached_symbol_path(cache_dir: &Path, app_path: &Path) -> PathBuf {
    let stem = app_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    cache_dir.join(format!("{}.symbols.app", stem))
}
