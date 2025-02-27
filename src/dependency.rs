use crate::config::BuildConfig;
use crate::error::{BuildError, BuildResult};
use log::{info, warn};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct DependencyManager {
    project_dir: PathBuf,
    deps_dir: PathBuf,
    config: Option<BuildConfig>,
}

impl DependencyManager {
    pub fn new(project_dir: &Path) -> Self {
        let deps_dir = project_dir.join("deps");

        DependencyManager {
            project_dir: project_dir.to_path_buf(),
            deps_dir,
            config: None,
        }
    }

    pub fn install(&mut self) -> BuildResult<()> {
        self.load_config()?;

        let config = self.config.as_ref().unwrap();
        if config.dependencies.is_empty() {
            println!("No dependencies to install.");
            return Ok(());
        }

        println!("Installing dependencies...");

        // 의존성 디렉토리 생성
        if !self.deps_dir.exists() {
            std::fs::create_dir_all(&self.deps_dir).map_err(|e| BuildError::IoError(e))?;
        }

        for (name, dep) in &config.dependencies {
            println!("Processing dependency: {}", name);

            let dep_dir = self.deps_dir.join(name);

            if dep_dir.exists() {
                info!(
                    "Dependency {} already installed at {}",
                    name,
                    dep_dir.display()
                );
                continue;
            }

            if let Some(ref git) = dep.git {
                self.install_git_dependency(name, git, &dep.branch, &dep.tag, &dep_dir)?;
            } else {
                warn!("Dependency {} has no source specified, skipping", name);
            }
        }

        println!("Dependencies installed successfully.");
        Ok(())
    }

    pub fn update(&mut self) -> BuildResult<()> {
        self.load_config()?;

        let config = self.config.as_ref().unwrap();
        if config.dependencies.is_empty() {
            println!("No dependencies to update.");
            return Ok(());
        }

        println!("Updating dependencies...");

        for (name, dep) in &config.dependencies {
            println!("Updating dependency: {}", name);

            let dep_dir = self.deps_dir.join(name);

            if !dep_dir.exists() {
                info!("Dependency {} not installed, installing fresh copy", name);
                if let Some(ref git) = dep.git {
                    self.install_git_dependency(name, git, &dep.branch, &dep.tag, &dep_dir)?;
                } else {
                    warn!("Dependency {} has no source specified, skipping", name);
                }
                continue;
            }

            if let Some(ref git) = dep.git {
                self.update_git_dependency(name, git, &dep.branch, &dep.tag, &dep_dir)?;
            } else {
                warn!("Dependency {} has no source specified, skipping", name);
            }
        }

        println!("Dependencies updated successfully.");
        Ok(())
    }

    fn install_git_dependency(
        &self,
        name: &str,
        git_url: &str,
        branch: &Option<String>,
        tag: &Option<String>,
        dep_dir: &Path,
    ) -> BuildResult<()> {
        info!("Cloning {} from {}", name, git_url);

        let mut cmd = Command::new("git");
        cmd.arg("clone").arg(git_url).arg(dep_dir);

        if let Some(ref branch) = branch {
            cmd.arg("--branch").arg(branch);
        }

        if let Some(ref tag) = tag {
            cmd.arg("--branch").arg(tag);
        }

        let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DependencyError(format!(
                "Failed to clone git repository for {}: {}",
                name, error
            )));
        }

        println!("Dependency {} installed successfully", name);
        Ok(())
    }

    fn update_git_dependency(
        &self,
        name: &str,
        git_url: &str,
        branch: &Option<String>,
        tag: &Option<String>,
        dep_dir: &Path,
    ) -> BuildResult<()> {
        info!("Updating {} from {}", name, git_url);

        // 리모트 원본 URL 확인
        let mut cmd = Command::new("git");
        cmd.current_dir(dep_dir)
            .arg("remote")
            .arg("get-url")
            .arg("origin");

        let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DependencyError(format!(
                "Failed to get remote URL for {}: {}",
                name, error
            )));
        }

        let current_url = String::from_utf8_lossy(&output.stdout).trim().to_string();

        // 원격 저장소 URL이 변경된 경우 업데이트
        if current_url != git_url {
            info!(
                "Remote URL changed for {}, updating from {} to {}",
                name, current_url, git_url
            );

            let mut cmd = Command::new("git");
            cmd.current_dir(dep_dir)
                .arg("remote")
                .arg("set-url")
                .arg("origin")
                .arg(git_url);

            let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(BuildError::DependencyError(format!(
                    "Failed to update remote URL for {}: {}",
                    name, error
                )));
            }
        }

        // 변경사항 가져오기
        let mut cmd = Command::new("git");
        cmd.current_dir(dep_dir).arg("fetch").arg("--all");

        let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DependencyError(format!(
                "Failed to fetch updates for {}: {}",
                name, error
            )));
        }

        // 브랜치나 태그로 체크아웃
        let checkout_target = if let Some(ref tag) = tag {
            tag.clone()
        } else if let Some(ref branch) = branch {
            format!("origin/{}", branch)
        } else {
            "origin/master".to_string()
        };

        let mut cmd = Command::new("git");
        cmd.current_dir(dep_dir)
            .arg("checkout")
            .arg(&checkout_target);

        let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to checkout {}: {}", checkout_target, error);

            // master/main으로 폴백
            info!("Trying to checkout master or main branch instead");

            for fallback in &["master", "main"] {
                let mut cmd = Command::new("git");
                cmd.current_dir(dep_dir)
                    .arg("checkout")
                    .arg(format!("origin/{}", fallback));

                let output = cmd.output();
                if let Ok(output) = output {
                    if output.status.success() {
                        info!("Successfully checked out {} branch", fallback);
                        break;
                    }
                }
            }
        }

        // 헤드 업데이트
        let mut cmd = Command::new("git");
        cmd.current_dir(dep_dir)
            .arg("reset")
            .arg("--hard")
            .arg("HEAD");

        let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(BuildError::DependencyError(format!(
                "Failed to reset to HEAD for {}: {}",
                name, error
            )));
        }

        println!("Dependency {} updated successfully", name);
        Ok(())
    }

    fn load_config(&mut self) -> BuildResult<()> {
        if self.config.is_none() {
            self.config = Some(BuildConfig::from_file(&self.project_dir)?);
        }
        Ok(())
    }

    pub fn get_include_paths(&mut self) -> BuildResult<Vec<PathBuf>> {
        self.load_config()?;

        let mut include_paths = Vec::new();

        for dep_name in self.config.as_ref().unwrap().dependencies.keys() {
            let dep_dir = self.deps_dir.join(dep_name);

            if !dep_dir.exists() {
                warn!("Dependency directory {} does not exist", dep_dir.display());
                continue;
            }

            // 일반적인 포함 디렉토리 패턴 추가
            for include_pattern in &["include", "inc", "headers"] {
                let include_dir = dep_dir.join(include_pattern);
                if include_dir.exists() && include_dir.is_dir() {
                    include_paths.push(include_dir);
                }
            }

            let dep_dir_clone = dep_dir.clone();

            for lib_name in &["lib", "src"] {
                let lib_dir = dep_dir_clone.join(lib_name);
                if lib_dir.exists() && lib_dir.is_dir() {
                    // 각 반복마다 dep_dir 참조를 안전하게 사용
                    let dep_dir_ref = dep_dir.clone();
                    for entry in std::fs::read_dir(&lib_dir)
                        .unwrap_or_else(|_| std::fs::read_dir(&dep_dir_ref).unwrap())
                    {
                        if let Ok(entry) = entry {
                            let entry_path = entry.path();
                            if entry_path.is_dir() {
                                let include_dir = entry_path.join("include");
                                if include_dir.exists() && include_dir.is_dir() {
                                    include_paths.push(include_dir);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(include_paths)
    }

    pub fn get_library_paths(&mut self) -> BuildResult<Vec<PathBuf>> {
        self.load_config()?;

        let mut lib_paths = Vec::new();

        for dep_name in self.config.as_ref().unwrap().dependencies.keys() {
            let dep_dir = self.deps_dir.join(dep_name);

            if !dep_dir.exists() {
                warn!("Dependency directory {} does not exist", dep_dir.display());
                continue;
            }

            // 일반적인 라이브러리 디렉토리 패턴 추가
            for lib_pattern in &["lib", "libs", "library", "libraries"] {
                let lib_dir = dep_dir.join(lib_pattern);
                if lib_dir.exists() && lib_dir.is_dir() {
                    lib_paths.push(lib_dir);
                }
            }

            // 빌드 결과물 디렉토리
            for build_pattern in &["build", "out", "output", "bin"] {
                // 각 패턴마다 새 build_dir 생성 (소유권 문제 해결)
                let build_dir = dep_dir.join(build_pattern);

                if build_dir.exists() && build_dir.is_dir() {
                    // 현재 build_dir을 lib_paths에 추가
                    lib_paths.push(build_dir.clone());

                    // build_dir의 클론으로 작업 (소유권 보존)
                    let build_dir_copy = build_dir.clone();

                    // 서브디렉토리 탐색
                    for lib_subdir in &["lib", "libs"] {
                        let lib_dir = build_dir_copy.join(lib_subdir);
                        if lib_dir.exists() && lib_dir.is_dir() {
                            lib_paths.push(lib_dir);
                        }
                    }
                }
            }
        }

        Ok(lib_paths)
    }
}
