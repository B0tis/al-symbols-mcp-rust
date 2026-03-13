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

pub struct AlCli {
    al_command: String,
}

#[derive(Debug, Clone)]
pub struct AlCliStatus {
    pub available: bool,
    pub path: String,
    pub version: Option<String>,
    pub message: String,
}

impl AlCli {
    /// Create a new AlCli instance, auto-detecting the AL command path.
    pub fn new() -> Self {
        let al_command = std::env::var("AL_CLI_PATH").unwrap_or_else(|_| "AL".into());
        Self { al_command }
    }

    /// Create with an explicit path to the AL binary.
    pub fn with_path(al_path: impl Into<String>) -> Self {
        Self {
            al_command: al_path.into(),
        }
    }

    /// Check if the AL CLI is available and return status info.
    pub fn check_availability(&self) -> AlCliStatus {
        match self.get_version() {
            Ok(version) => AlCliStatus {
                available: true,
                path: self.al_command.clone(),
                version: Some(version.clone()),
                message: format!("AL CLI available: {} ({})", self.al_command, version),
            },
            Err(_) => {
                if let Some(found) = self.search_common_paths() {
                    match Self::with_path(&found).get_version() {
                        Ok(version) => AlCliStatus {
                            available: true,
                            path: found,
                            version: Some(version.clone()),
                            message: format!("AL CLI found at alternate path ({})", version),
                        },
                        Err(_) => AlCliStatus {
                            available: false,
                            path: self.al_command.clone(),
                            version: None,
                            message: Self::install_instructions(),
                        },
                    }
                } else {
                    AlCliStatus {
                        available: false,
                        path: self.al_command.clone(),
                        version: None,
                        message: Self::install_instructions(),
                    }
                }
            }
        }
    }

    /// Try to find AL CLI and update internal path. Returns the resolved AlCli if found.
    pub fn resolve(mut self) -> Self {
        if self.get_version().is_ok() {
            return self;
        }
        if let Some(found) = self.search_common_paths() {
            if Self::with_path(&found).get_version().is_ok() {
                self.al_command = found;
            }
        }
        self
    }

    /// Check if AL CLI is available (quick test).
    pub fn is_available(&self) -> bool {
        self.get_version().is_ok()
    }

    /// Get the AL CLI version string.
    pub fn get_version(&self) -> Result<String, AlCliError> {
        let output = Command::new(&self.al_command)
            .arg("--version")
            .output()
            .map_err(AlCliError::ProcessError)?;

        if output.status.success() || !output.stdout.is_empty() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if version.is_empty() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if !stderr.is_empty() {
                    return Ok(stderr);
                }
            }
            Ok(version)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(AlCliError::CommandFailed {
                code: output.status.code().unwrap_or(-1),
                stderr,
            })
        }
    }

    /// Convert a runtime .app package into a symbol package that contains SymbolReference.json.
    /// Returns the path to the created symbol package (temporary file).
    pub fn create_symbol_package(&self, app_path: &Path) -> Result<PathBuf, AlCliError> {
        let symbol_path = std::env::temp_dir().join(format!(
            "al_symbols_{}_{}",
            std::process::id(),
            app_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
        )).with_extension("app");

        debug!(
            "AL CLI: CreateSymbolPackage {} -> {}",
            app_path.display(),
            symbol_path.display()
        );

        let output = Command::new(&self.al_command)
            .arg("CreateSymbolPackage")
            .arg(app_path)
            .arg(&symbol_path)
            .output()
            .map_err(AlCliError::ProcessError)?;

        if output.status.success() {
            if symbol_path.exists() {
                info!(
                    "AL CLI: Created symbol package at {}",
                    symbol_path.display()
                );
                Ok(symbol_path)
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
        if !Self::is_dotnet_available() {
            return Err(AlCliError::DotnetNotFound);
        }

        let package_name = Self::platform_package_name();
        info!("Attempting to install AL CLI: dotnet tool install --global {} --prerelease", package_name);

        let output = Command::new("dotnet")
            .args(["tool", "install", "--global", &package_name, "--prerelease"])
            .output()
            .map_err(AlCliError::ProcessError)?;

        if output.status.success() {
            let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
            info!("AL CLI installed successfully: {}", msg);
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

    /// Get OS-specific dotnet package name for the AL CLI.
    fn platform_package_name() -> String {
        if cfg!(target_os = "windows") {
            "Microsoft.Dynamics.BusinessCentral.Development.Tools".into()
        } else if cfg!(target_os = "macos") {
            "Microsoft.Dynamics.BusinessCentral.Development.Tools.Osx".into()
        } else {
            "Microsoft.Dynamics.BusinessCentral.Development.Tools.Linux".into()
        }
    }

    fn is_dotnet_available() -> bool {
        Command::new("dotnet")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn search_common_paths(&self) -> Option<String> {
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

        for candidate in candidates {
            if candidate.exists() {
                let path_str = candidate.to_string_lossy().to_string();
                debug!("Found AL CLI candidate at {}", path_str);
                return Some(path_str);
            }
        }
        None
    }

    pub fn install_instructions() -> String {
        let pkg = Self::platform_package_name();
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

    /// Clean up a temporary symbol package file.
    pub fn cleanup_symbol_file(path: &Path) {
        if path.exists() {
            if let Err(e) = std::fs::remove_file(path) {
                warn!("Failed to clean up symbol file {}: {}", path.display(), e);
            }
        }
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}
