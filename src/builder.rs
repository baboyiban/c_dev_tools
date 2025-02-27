use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use colored::Colorize;
use dashmap::DashMap;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::{debug, error, info, warn};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::config::{BuildConfig, ExecutableTarget, LibraryTarget, TestTarget};
use crate::error::{BuildError, BuildResult};
use crate::parser::DependencyParser;

pub struct Builder {
    project_dir: PathBuf,
    build_dir: PathBuf,
    config: Option<BuildConfig>,
    configuration: String,
    jobs: usize,
    incremental: bool,
    verbose: bool,
}

impl Builder {
    pub fn new(project_dir: &Path, configuration: &str, jobs: usize) -> Self {
        let build_dir = project_dir.join("build").join(configuration);

        Builder {
            project_dir: project_dir.to_path_buf(),
            build_dir,
            config: None,
            configuration: configuration.to_string(),
            jobs,
            incremental: false,
            verbose: false,
        }
    }

    pub fn set_incremental(&mut self, incremental: bool) {
        self.incremental = incremental;
    }

    pub fn set_verbose(&mut self, verbose: bool) {
        self.verbose = verbose;
    }

    pub fn build(&mut self) -> BuildResult<()> {
        let start_time = Instant::now();
        info!("빌드 시작: {}", self.project_dir.display());

        // 설정 로드
        self.config = Some(BuildConfig::from_file(&self.project_dir)?);
        let config = self.config.as_ref().unwrap();

        println!(
            "{} {} v{}",
            "Building".green().bold(),
            config.project.name,
            config.project.version
        );

        // 컴파일러 확인
        self.check_compiler()?;

        // 빌드 디렉토리 준비
        self.prepare_build_directory()?;

        // 소스 파일 해결
        let source_files = config.resolve_source_files(&self.project_dir)?;

        // 파일 변경 검사 (증분 빌드)
        let changed_files = if self.incremental {
            self.detect_changed_files(&source_files)?
        } else {
            // 증분 빌드가 아니면 모든 파일을 변경된 것으로 간주
            source_files.clone()
        };

        if changed_files.is_empty() {
            println!("{}", "모든 파일이 최신 상태입니다.".green());
            return Ok(());
        }

        // 컴파일
        self.compile_sources(&changed_files)?;

        // 링크
        self.link_targets()?;

        let duration = start_time.elapsed();
        println!(
            "{} ({}초)",
            "빌드 완료".green().bold(),
            duration.as_secs_f32()
        );

        Ok(())
    }

    pub fn clean(&self) -> BuildResult<()> {
        info!("정리 중: {}", self.build_dir.display());

        if self.build_dir.exists() {
            std::fs::remove_dir_all(&self.build_dir).map_err(|e| BuildError::IoError(e))?;
            println!("{}: {}", "정리 완료".green(), self.build_dir.display());
        } else {
            println!("{}: 이미 정리되어 있습니다", "정리".green());
        }

        Ok(())
    }

    fn check_compiler(&self) -> BuildResult<()> {
        let config = self.config.as_ref().unwrap();
        let compiler = &config.build.compiler;

        let output = Command::new(compiler).arg("--version").output();

        match output {
            Ok(output) if output.status.success() => {
                let version = String::from_utf8_lossy(&output.stdout);
                info!(
                    "컴파일러: {} ({})",
                    compiler,
                    version.lines().next().unwrap_or("")
                );
                Ok(())
            }
            _ => Err(BuildError::CompilerNotFound(compiler.clone())),
        }
    }

    fn prepare_build_directory(&self) -> BuildResult<()> {
        if !self.build_dir.exists() {
            std::fs::create_dir_all(&self.build_dir).map_err(|e| BuildError::IoError(e))?;
        }

        // 객체 파일 디렉토리
        let obj_dir = self.build_dir.join("obj");
        if !obj_dir.exists() {
            std::fs::create_dir_all(&obj_dir).map_err(|e| BuildError::IoError(e))?;
        }

        // 라이브러리 디렉토리
        let lib_dir = self.build_dir.join("lib");
        if !lib_dir.exists() {
            std::fs::create_dir_all(&lib_dir).map_err(|e| BuildError::IoError(e))?;
        }

        // 실행 파일 디렉토리
        let bin_dir = self.build_dir.join("bin");
        if !bin_dir.exists() {
            std::fs::create_dir_all(&bin_dir).map_err(|e| BuildError::IoError(e))?;
        }

        Ok(())
    }

    fn detect_changed_files(
        &self,
        source_files: &HashMap<String, Vec<PathBuf>>,
    ) -> BuildResult<HashMap<String, Vec<PathBuf>>> {
        let mut changed_files = HashMap::new();
        let hash_file = self.build_dir.join("file_hashes.json");

        // 이전 해시 로드
        let mut previous_hashes: HashMap<String, String> = if hash_file.exists() {
            let content =
                std::fs::read_to_string(&hash_file).map_err(|e| BuildError::IoError(e))?;
            serde_json::from_str(&content)
                .map_err(|e| BuildError::ConfigParsingError(e.to_string()))?
        } else {
            HashMap::new()
        };

        // 변경된 파일 감지 및 새 해시 계산
        let mut new_hashes = HashMap::new();

        for (target, files) in source_files {
            let mut changed = Vec::new();

            for file in files {
                let path_str = file.to_string_lossy().to_string();
                let hash = calculate_file_hash(file)?;

                if !previous_hashes.contains_key(&path_str)
                    || previous_hashes.get(&path_str) != Some(&hash)
                {
                    changed.push(file.clone());
                }

                new_hashes.insert(path_str, hash);
            }

            if !changed.is_empty() {
                changed_files.insert(target.clone(), changed);
            }
        }

        // 새 해시 저장
        let json = serde_json::to_string(&new_hashes)
            .map_err(|e| BuildError::ConfigParsingError(e.to_string()))?;
        std::fs::write(&hash_file, json).map_err(|e| BuildError::IoError(e))?;

        Ok(changed_files)
    }

    fn compile_sources(&self, source_files: &HashMap<String, Vec<PathBuf>>) -> BuildResult<()> {
        if source_files.is_empty() {
            return Ok(());
        }

        let config = self.config.as_ref().unwrap();
        let compiler = &config.build.compiler;

        println!(
            "{} ({}개 작업, {}개 스레드 사용)",
            "컴파일 중".blue().bold(),
            source_files.values().map(|v| v.len()).sum::<usize>(),
            self.jobs
        );

        let mp = MultiProgress::new();
        let sty = ProgressStyle::default_bar()
            .template("{prefix:.bold.dim} [{bar:40}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=> ");

        let total_files: usize = source_files.values().map(|v| v.len()).sum();
        let total_pb = mp.add(ProgressBar::new(total_files as u64));
        total_pb.set_style(sty.clone());

        total_pb.set_prefix("[전체]".to_string());

        let error_map: Arc<DashMap<PathBuf, String>> = Arc::new(DashMap::new());
        let total_pb_arc = Arc::new(total_pb);

        // 먼저 모든 타겟과 파일 개수를 수집하고 프로그레스바 미리 생성
        let mut target_progress_bars = Vec::new();
        for (target_name, files) in source_files {
            let target_parts: Vec<&str> = target_name.splitn(2, ':').collect();
            let target_type = target_parts[0];
            let target_name = target_parts[1];

            // 이 문자열은 Vec에 저장되므로 함수가 반환될 때까지 유효함
            let prefix = format!("[{}]", target_name);
            let target_pb = mp.add(ProgressBar::new(files.len() as u64));
            target_pb.set_style(sty.clone());
            target_pb.set_prefix(prefix);

            target_progress_bars.push((
                target_name.to_string(),
                target_type.to_string(),
                Arc::new(target_pb),
                files,
            ));
        }

        // 이제 실제 컴파일 수행
        let compile_results: Vec<BuildResult<()>> = target_progress_bars
            .into_par_iter() // 소유권 이전
            .flat_map(|(target_name, target_type, target_pb_arc, files)| {
                let error_map_arc = error_map.clone();
                let total_pb_arc_clone = total_pb_arc.clone();

                files
                    .par_iter()
                    .map(move |source_file| {
                        let pb = target_pb_arc.clone();
                        let err_map = error_map_arc.clone();
                        let total = total_pb_arc_clone.clone();

                        // 출력 경로 생성
                        let rel_path = source_file
                            .strip_prefix(&self.project_dir)
                            .unwrap_or(source_file);
                        let object_file = self
                            .build_dir
                            .join("obj")
                            .join(rel_path)
                            .with_extension("o");

                        // 객체 파일 디렉토리 생성
                        if let Some(parent) = object_file.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| BuildError::IoError(e))?;
                        }

                        // 컴파일 플래그 설정
                        let mut cmd = Command::new(compiler);
                        cmd.arg("-c").arg(source_file).arg("-o").arg(&object_file);

                        // 표준 설정
                        if let Some(ref c_std) = config.build.c_standard {
                            cmd.arg(format!("-std={}", c_std));
                        }

                        // 최적화 수준
                        if let Some(opt_level) = config.build.optimization_level {
                            cmd.arg(format!("-O{}", opt_level));
                        }

                        // 디버그 정보
                        if config.build.debug_info.unwrap_or(false) {
                            cmd.arg("-g");
                        }

                        // 경고를 오류로 처리
                        if config.build.warnings_as_errors.unwrap_or(false) {
                            cmd.arg("-Werror");
                        }

                        // 포함 디렉토리 추가
                        let include_dirs = match target_type.as_str() {
                            "exe" => config
                                .targets
                                .executable
                                .iter()
                                .find(|t| t.name == target_name)
                                .and_then(|t| t.include_dirs.clone())
                                .unwrap_or_default(),
                            "static" | "shared" => {
                                let libs = if target_type == "static" {
                                    &config.targets.static_lib
                                } else {
                                    &config.targets.shared_lib
                                };

                                libs.iter()
                                    .find(|t| t.name == target_name)
                                    .and_then(|t| t.include_dirs.clone())
                                    .unwrap_or_default()
                            }
                            "test" => config
                                .targets
                                .test
                                .iter()
                                .find(|t| t.name == target_name)
                                .and_then(|t| t.include_dirs.clone())
                                .unwrap_or_default(),
                            _ => Vec::new(),
                        };

                        for dir in include_dirs {
                            let include_path = if Path::new(&dir).is_absolute() {
                                PathBuf::from(dir)
                            } else {
                                self.project_dir.join(dir)
                            };
                            cmd.arg("-I").arg(include_path);
                        }

                        // 매크로 정의 추가
                        let defines: HashMap<String, String> = match target_type.as_str() {
                            "exe" => config
                                .targets
                                .executable
                                .iter()
                                .find(|t| t.name == target_name)
                                .and_then(|t| t.defines.clone())
                                .unwrap_or_default(),
                            "static" | "shared" => {
                                let libs = if target_type == "static" {
                                    &config.targets.static_lib
                                } else {
                                    &config.targets.shared_lib
                                };

                                libs.iter()
                                    .find(|t| t.name == target_name)
                                    .and_then(|t| t.defines.clone())
                                    .unwrap_or_default()
                            }
                            "test" => config
                                .targets
                                .test
                                .iter()
                                .find(|t| t.name == target_name)
                                .and_then(|t| t.defines.clone())
                                .unwrap_or_default(),
                            _ => HashMap::new(),
                        };

                        for (key, value) in defines {
                            if value.is_empty() {
                                cmd.arg(format!("-D{}", key));
                            } else {
                                cmd.arg(format!("-D{}={}", key, value));
                            }
                        }

                        // 추가 플래그 추가
                        let extra_flags: Vec<String> = match target_type.as_str() {
                            "exe" => config
                                .targets
                                .executable
                                .iter()
                                .find(|t| t.name == target_name)
                                .and_then(|t| t.extra_flags.clone())
                                .unwrap_or_default(),
                            "static" | "shared" => {
                                let libs = if target_type == "static" {
                                    &config.targets.static_lib
                                } else {
                                    &config.targets.shared_lib
                                };

                                libs.iter()
                                    .find(|t| t.name == target_name)
                                    .and_then(|t| t.extra_flags.clone())
                                    .unwrap_or_default()
                            }
                            "test" => config
                                .targets
                                .test
                                .iter()
                                .find(|t| t.name == target_name)
                                .and_then(|t| t.extra_flags.clone())
                                .unwrap_or_default(),
                            _ => Vec::new(),
                        };

                        for flag in extra_flags {
                            cmd.arg(flag);
                        }

                        // 빌드 구성에 따른 추가 설정
                        if self.configuration == "release" {
                            cmd.arg("-DNDEBUG");
                        } else {
                            cmd.arg("-D_DEBUG");
                        }

                        // PIC (Position Independent Code) 옵션 - 공유 라이브러리용
                        if target_type == "shared" {
                            cmd.arg("-fPIC");
                        }

                        // 전역 추가 플래그
                        if let Some(ref extra_flags) = config.build.extra_flags {
                            for flag in extra_flags {
                                cmd.arg(flag);
                            }
                        }

                        if self.verbose {
                            println!("Compiling: {:?}", cmd);
                        }

                        // 파일 이름 문자열 생성 및 메시지 설정
                        let file_name = source_file
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        pb.set_message(file_name);

                        // 컴파일 실행
                        let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

                        if !output.status.success() {
                            let error_msg = String::from_utf8_lossy(&output.stderr).to_string();
                            err_map.insert(source_file.clone(), error_msg);
                            return Err(BuildError::CompilerError(format!(
                                "컴파일 실패: {}",
                                source_file.display()
                            )));
                        }

                        pb.inc(1);
                        total.inc(1);

                        Ok(())
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        // 컴파일 오류 출력
        if !error_map.is_empty() {
            println!("\n{}", "컴파일 오류:".red().bold());
            for entry in error_map.iter() {
                println!(
                    "{}: \n{}",
                    entry.key().display().to_string().yellow(),
                    entry.value()
                );
            }
            return Err(BuildError::CompilerError("컴파일 오류 발생".to_string()));
        }

        // 컴파일 성공 여부 확인
        let has_errors = compile_results.iter().any(|r| r.is_err());
        if has_errors {
            return Err(BuildError::CompilerError("빌드 실패".to_string()));
        }

        println!("{}", "컴파일 완료".green());
        Ok(())
    }

    fn link_targets(&self) -> BuildResult<()> {
        println!("{}", "링크 중...".blue().bold());

        let config = self.config.as_ref().unwrap();
        let compiler = &config.build.compiler;

        // 정적 라이브러리 링크
        self.link_static_libraries()?;

        // 공유 라이브러리 링크
        self.link_shared_libraries()?;

        // 실행 파일 링크
        self.link_executables()?;

        // 테스트 링크
        self.link_tests()?;

        println!("{}", "링크 완료".green());
        Ok(())
    }

    fn link_static_libraries(&self) -> BuildResult<()> {
        let config = self.config.as_ref().unwrap();

        if config.targets.static_lib.is_empty() {
            return Ok(());
        }

        for lib in &config.targets.static_lib {
            println!("Static library: {}", lib.name);

            let source_files = config
                .resolve_source_files(&self.project_dir)?
                .get(&format!("static:{}", lib.name))
                .cloned()
                .unwrap_or_default();

            if source_files.is_empty() {
                warn!("No source files for static library: {}", lib.name);
                continue;
            }

            // 객체 파일 수집
            let mut object_files = Vec::new();
            for source in &source_files {
                let rel_path = source.strip_prefix(&self.project_dir).unwrap_or(source);
                let object_file = self
                    .build_dir
                    .join("obj")
                    .join(rel_path)
                    .with_extension("o");

                if !object_file.exists() {
                    warn!("Object file does not exist: {}", object_file.display());
                    continue;
                }

                object_files.push(object_file);
            }

            if object_files.is_empty() {
                warn!("No object files found for static library: {}", lib.name);
                continue;
            }

            // 라이브러리 파일 경로
            let lib_name = format!("lib{}.a", lib.name);
            let lib_path = self.build_dir.join("lib").join(&lib_name);

            // 아카이버 실행
            let mut cmd = Command::new("ar");
            cmd.arg("rcs").arg(&lib_path);

            for obj in &object_files {
                cmd.arg(obj);
            }

            if self.verbose {
                println!("Archiving: {:?}", cmd);
            }

            let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

            // 오류 처리 시 원본 사용 (이동되지 않음)
            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                let message = format!("Failed to create static library: {} - {}", lib_name, error);
                return Err(BuildError::LinkerError(message));
            }

            println!(
                "{} {}",
                "Created static library:".green(),
                lib_path.display()
            );
        }

        Ok(())
    }

    fn link_shared_libraries(&self) -> BuildResult<()> {
        let config = self.config.as_ref().unwrap();
        let compiler = &config.build.compiler;

        if config.targets.shared_lib.is_empty() {
            return Ok(());
        }

        for lib in &config.targets.shared_lib {
            println!("Shared library: {}", lib.name);

            let source_files = config
                .resolve_source_files(&self.project_dir)?
                .get(&format!("shared:{}", lib.name))
                .cloned()
                .unwrap_or_default();

            if source_files.is_empty() {
                warn!("No source files for shared library: {}", lib.name);
                continue;
            }

            // 객체 파일 수집
            let mut object_files = Vec::new();
            for source in &source_files {
                let rel_path = source.strip_prefix(&self.project_dir).unwrap_or(source);
                let object_file = self
                    .build_dir
                    .join("obj")
                    .join(rel_path)
                    .with_extension("o");

                if !object_file.exists() {
                    warn!("Object file does not exist: {}", object_file.display());
                    continue;
                }

                object_files.push(object_file);
            }

            if object_files.is_empty() {
                warn!("No object files found for shared library: {}", lib.name);
                continue;
            }

            // 라이브러리 파일 경로
            let lib_name = if cfg!(target_os = "windows") {
                format!("{}.dll", lib.name)
            } else if cfg!(target_os = "macos") {
                format!("lib{}.dylib", lib.name)
            } else {
                format!("lib{}.so", lib.name)
            };

            let lib_path = self.build_dir.join("lib").join(&lib_name);

            // 링커 실행
            let mut cmd = Command::new(compiler);
            cmd.arg("-shared").arg("-o").arg(&lib_path);

            for obj in &object_files {
                cmd.arg(obj);
            }

            // macOS 설정
            if cfg!(target_os = "macos") {
                cmd.arg("-install_name").arg(format!("@rpath/{}", lib_name));
            }

            if self.configuration == "release" {
                if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
                    cmd.arg("-s"); // 심볼 정보 제거 (스트립)
                }
            }

            if self.verbose {
                println!("Linking shared library: {:?}", cmd);
            }

            let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(BuildError::LinkerError(format!(
                    "Failed to create shared library: {} - {}",
                    lib_name, error
                )));
            }

            println!(
                "{} {}",
                "Created shared library:".green(),
                lib_path.display()
            );
        }

        Ok(())
    }

    fn link_executables(&self) -> BuildResult<()> {
        let config = self.config.as_ref().unwrap();
        let compiler = &config.build.compiler;

        if config.targets.executable.is_empty() {
            return Ok(());
        }

        for exe in &config.targets.executable {
            println!("Executable: {}", exe.name);

            let source_files = config
                .resolve_source_files(&self.project_dir)?
                .get(&format!("exe:{}", exe.name))
                .cloned()
                .unwrap_or_default();

            if source_files.is_empty() {
                warn!("No source files for executable: {}", exe.name);
                continue;
            }

            // 객체 파일 수집
            let mut object_files = Vec::new();
            for source in &source_files {
                let rel_path = source.strip_prefix(&self.project_dir).unwrap_or(source);
                let object_file = self
                    .build_dir
                    .join("obj")
                    .join(rel_path)
                    .with_extension("o");

                if !object_file.exists() {
                    warn!("Object file does not exist: {}", object_file.display());
                    continue;
                }

                object_files.push(object_file);
            }

            if object_files.is_empty() {
                warn!("No object files found for executable: {}", exe.name);
                continue;
            }

            // 실행 파일 경로
            let exe_name = if cfg!(target_os = "windows") {
                format!("{}.exe", exe.name)
            } else {
                exe.name.clone()
            };

            let exe_path = self.build_dir.join("bin").join(exe_name);

            // 링커 실행
            let mut cmd = Command::new(compiler);
            cmd.arg("-o").arg(&exe_path);

            for obj in &object_files {
                cmd.arg(obj);
            }

            // 라이브러리 경로 추가
            if let Some(link_dirs) = &exe.link_dirs {
                for dir in link_dirs {
                    let link_path = if Path::new(dir).is_absolute() {
                        PathBuf::from(dir)
                    } else {
                        self.project_dir.join(dir)
                    };
                    cmd.arg("-L").arg(link_path);
                }
            }

            // 내부 정적 라이브러리 추가
            for static_lib in &config.targets.static_lib {
                let lib_path = self
                    .build_dir
                    .join("lib")
                    .join(format!("lib{}.a", static_lib.name));

                if lib_path.exists() {
                    cmd.arg(lib_path);
                }
            }

            // 내부 공유 라이브러리 경로 추가
            cmd.arg("-L").arg(self.build_dir.join("lib"));

            // 라이브러리 추가
            if let Some(libs) = &exe.libs {
                for lib in libs {
                    cmd.arg(format!("-l{}", lib));
                }
            }

            // rpath 설정 (공유 라이브러리 위치 보존)
            if cfg!(target_os = "linux") {
                cmd.arg(format!(
                    "-Wl,-rpath,{}",
                    self.build_dir.join("lib").display()
                ));
            } else if cfg!(target_os = "macos") {
                cmd.arg("-Wl,-rpath,@executable_path/../lib");
            }

            if self.configuration == "release" {
                if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
                    cmd.arg("-s"); // 심볼 정보 제거 (스트립)
                }
            }

            if self.verbose {
                println!("Linking executable: {:?}", cmd);
            }

            let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(BuildError::LinkerError(format!(
                    "Failed to create executable: {} - {}",
                    exe.name, error
                )));
            }

            println!("{} {}", "Created executable:".green(), exe_path.display());
        }

        Ok(())
    }

    fn link_tests(&self) -> BuildResult<()> {
        let config = self.config.as_ref().unwrap();
        let compiler = &config.build.compiler;

        if config.targets.test.is_empty() {
            return Ok(());
        }

        for test in &config.targets.test {
            println!("Test executable: {}", test.name);

            let source_files = config
                .resolve_source_files(&self.project_dir)?
                .get(&format!("test:{}", test.name))
                .cloned()
                .unwrap_or_default();

            if source_files.is_empty() {
                warn!("No source files for test: {}", test.name);
                continue;
            }

            // 객체 파일 수집
            let mut object_files = Vec::new();
            for source in &source_files {
                let rel_path = source.strip_prefix(&self.project_dir).unwrap_or(source);
                let object_file = self
                    .build_dir
                    .join("obj")
                    .join(rel_path)
                    .with_extension("o");

                if !object_file.exists() {
                    warn!("Object file does not exist: {}", object_file.display());
                    continue;
                }

                object_files.push(object_file);
            }

            if object_files.is_empty() {
                warn!("No object files found for test: {}", test.name);
                continue;
            }

            // 테스트 실행 파일 경로
            let test_name = if cfg!(target_os = "windows") {
                format!("{}.exe", test.name)
            } else {
                test.name.clone()
            };

            let test_path = self.build_dir.join("bin").join("tests").join(&test_name);

            // 테스트 디렉토리 생성
            if let Some(parent) = test_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| BuildError::IoError(e))?;
            }

            // 링커 실행
            let mut cmd = Command::new(compiler);
            cmd.arg("-o").arg(&test_path);

            for obj in &object_files {
                cmd.arg(obj);
            }

            // 라이브러리 경로 추가
            if let Some(link_dirs) = &test.link_dirs {
                for dir in link_dirs {
                    let link_path = if Path::new(dir).is_absolute() {
                        PathBuf::from(dir)
                    } else {
                        self.project_dir.join(dir)
                    };
                    cmd.arg("-L").arg(link_path);
                }
            }

            // 내부 정적 라이브러리 추가
            for static_lib in &config.targets.static_lib {
                let lib_path = self
                    .build_dir
                    .join("lib")
                    .join(format!("lib{}.a", static_lib.name));

                if lib_path.exists() {
                    cmd.arg(lib_path);
                }
            }

            // 내부 공유 라이브러리 경로 추가
            cmd.arg("-L").arg(self.build_dir.join("lib"));

            // 라이브러리 추가
            if let Some(libs) = &test.libs {
                for lib in libs {
                    cmd.arg(format!("-l{}", lib));
                }
            }

            // rpath 설정 (공유 라이브러리 위치 보존)
            if cfg!(target_os = "linux") {
                cmd.arg(format!(
                    "-Wl,-rpath,{}",
                    self.build_dir.join("lib").display()
                ));
            } else if cfg!(target_os = "macos") {
                cmd.arg("-Wl,-rpath,@executable_path/../../lib");
            }

            if self.verbose {
                println!("Linking test: {:?}", cmd);
            }

            let output = cmd.output().map_err(|e| BuildError::IoError(e))?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(BuildError::LinkerError(format!(
                    "Failed to create test executable: {} - {}",
                    test.name, error
                )));
            }

            println!(
                "{} {}",
                "Created test executable:".green(),
                test_path.display()
            );
        }

        Ok(())
    }
}

fn calculate_file_hash(path: &Path) -> BuildResult<String> {
    let mut file = std::fs::File::open(path).map_err(|e| BuildError::IoError(e))?;

    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| BuildError::IoError(e))?;

    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}
