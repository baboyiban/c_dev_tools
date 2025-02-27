use crate::error::{BuildError, BuildResult};
use camino::Utf8Path;
use glob::glob;
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProjectInfo {
    pub name: String,
    pub version: String,
    pub authors: Option<Vec<String>>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BuildSettings {
    pub compiler: String,
    pub c_standard: Option<String>,
    pub cpp_standard: Option<String>,
    pub optimization_level: Option<u8>,
    pub debug_info: Option<bool>,
    pub warnings_as_errors: Option<bool>,
    pub extra_flags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Dependency {
    pub version: String,
    pub features: Option<Vec<String>>,
    pub git: Option<String>,
    pub tag: Option<String>,
    pub branch: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ExecutableTarget {
    pub name: String,
    pub src: Vec<String>,
    pub include_dirs: Option<Vec<String>>,
    pub link_dirs: Option<Vec<String>>,
    pub libs: Option<Vec<String>>,
    pub defines: Option<HashMap<String, String>>,
    pub extra_flags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LibraryTarget {
    pub name: String,
    pub src: Vec<String>,
    pub include_dirs: Option<Vec<String>>,
    pub defines: Option<HashMap<String, String>>,
    pub extra_flags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TestTarget {
    pub name: String,
    pub src: Vec<String>,
    pub include_dirs: Option<Vec<String>>,
    pub link_dirs: Option<Vec<String>>,
    pub libs: Option<Vec<String>>,
    pub defines: Option<HashMap<String, String>>,
    pub extra_flags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Targets {
    #[serde(default)]
    pub executable: Vec<ExecutableTarget>,
    #[serde(default)]
    pub static_lib: Vec<LibraryTarget>,
    #[serde(default)]
    pub shared_lib: Vec<LibraryTarget>,
    #[serde(default)]
    pub test: Vec<TestTarget>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BuildConfig {
    pub project: ProjectInfo,
    pub build: BuildSettings,
    #[serde(default)]
    pub dependencies: HashMap<String, Dependency>,
    pub targets: Targets,
}

impl BuildConfig {
    pub fn from_file(path: &Path) -> BuildResult<Self> {
        let config_path = path.join("cbuild.toml");

        if !config_path.exists() {
            return Err(BuildError::ConfigNotFound(config_path));
        }

        info!("설정 파일 로드 중: {}", config_path.display());

        let content = std::fs::read_to_string(&config_path).map_err(|e| BuildError::IoError(e))?;

        let config: BuildConfig =
            toml::from_str(&content).map_err(|e| BuildError::ConfigParsingError(e.to_string()))?;

        Ok(config)
    }

    pub fn resolve_source_files(
        &self,
        project_dir: &Path,
    ) -> BuildResult<HashMap<String, Vec<PathBuf>>> {
        let mut resolved_sources = HashMap::new();

        // 실행 파일 소스 해결
        for target in &self.targets.executable {
            let sources = resolve_glob_patterns(&target.src, project_dir)?;
            resolved_sources.insert(format!("exe:{}", target.name), sources);
        }

        // 정적 라이브러리 소스 해결
        for target in &self.targets.static_lib {
            let sources = resolve_glob_patterns(&target.src, project_dir)?;
            resolved_sources.insert(format!("static:{}", target.name), sources);
        }

        // 공유 라이브러리 소스 해결
        for target in &self.targets.shared_lib {
            let sources = resolve_glob_patterns(&target.src, project_dir)?;
            resolved_sources.insert(format!("shared:{}", target.name), sources);
        }

        // 테스트 소스 해결
        for target in &self.targets.test {
            let sources = resolve_glob_patterns(&target.src, project_dir)?;
            resolved_sources.insert(format!("test:{}", target.name), sources);
        }

        Ok(resolved_sources)
    }
}

fn resolve_glob_patterns(patterns: &[String], base_dir: &Path) -> BuildResult<Vec<PathBuf>> {
    let mut resolved_files = Vec::new();

    for pattern in patterns {
        let pattern_path = if Path::new(pattern).is_absolute() {
            pattern.clone()
        } else {
            base_dir.join(pattern).to_string_lossy().to_string()
        };

        let paths = glob(&pattern_path)
            .map_err(|e| BuildError::PathError(format!("패턴 '{}'에 오류: {}", pattern, e)))?;

        for path in paths {
            match path {
                Ok(path) => resolved_files.push(path),
                Err(e) => return Err(BuildError::PathError(format!("경로 해결 오류: {}", e))),
            }
        }
    }

    if resolved_files.is_empty() {
        return Err(BuildError::NoSourceFiles(patterns.join(", ")));
    }

    Ok(resolved_files)
}
