use crate::config::BuildConfig;
use crate::error::{BuildError, BuildResult};
use colored::Colorize;
use log::{error, info};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Runner {
    project_dir: PathBuf,
    build_dir: PathBuf,
    config: Option<BuildConfig>,
}

impl Runner {
    pub fn new(project_dir: &Path) -> Self {
        let build_dir = project_dir.join("build").join("debug");

        Runner {
            project_dir: project_dir.to_path_buf(),
            build_dir,
            config: None,
        }
    }

    pub fn run(&self, args: Option<&str>) -> BuildResult<()> {
        // 설정 로드
        let config = self.load_config()?;

        // 실행할 타겟 찾기
        if config.targets.executable.is_empty() {
            return Err(BuildError::ExecutableNotFound(self.build_dir.join("bin")));
        }

        // 메인 실행 파일 결정 (첫 번째 또는 프로젝트 이름과 일치하는 것)
        let main_exe = config
            .targets
            .executable
            .iter()
            .find(|exe| exe.name == config.project.name)
            .or_else(|| config.targets.executable.first())
            .unwrap();

        // 실행 파일 경로
        let exe_name = if cfg!(target_os = "windows") {
            format!("{}.exe", main_exe.name)
        } else {
            main_exe.name.clone()
        };

        let exe_path = self.build_dir.join("bin").join(&exe_name);

        if !exe_path.exists() {
            return Err(BuildError::ExecutableNotFound(exe_path));
        }

        println!("{} {}", "Running".green().bold(), exe_path.display());

        // 실행 명령 생성
        let mut cmd = Command::new(&exe_path);

        // 인자 추가
        if let Some(args_str) = args {
            for arg in args_str.split_whitespace() {
                cmd.arg(arg);
            }
        }

        // 환경 변수 설정: 공유 라이브러리 경로
        if cfg!(target_os = "linux") {
            let lib_path = self.build_dir.join("lib");
            cmd.env("LD_LIBRARY_PATH", &lib_path);
        } else if cfg!(target_os = "macos") {
            let lib_path = self.build_dir.join("lib");
            cmd.env("DYLD_LIBRARY_PATH", &lib_path);
        } else if cfg!(target_os = "windows") {
            let lib_path = self.build_dir.join("lib");
            cmd.env(
                "PATH",
                format!(
                    "{};{}",
                    lib_path.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            );
        }

        // 프로그램 실행
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        info!("Executing: {:?}", cmd);

        match cmd.status() {
            Ok(status) => {
                if status.success() {
                    println!("{}", "Program executed successfully".green());
                } else {
                    let code = status.code().unwrap_or(-1);
                    println!("{} (code: {})", "Program exited with error".red(), code);
                }
                Ok(())
            }
            Err(e) => {
                error!("Failed to execute program: {}", e);
                Err(BuildError::IoError(e))
            }
        }
    }

    pub fn run_tests(&self) -> BuildResult<()> {
        // 설정 로드
        let config = self.load_config()?;

        // 테스트 타겟 찾기
        if config.targets.test.is_empty() {
            println!("No test targets found.");
            return Ok(());
        }

        println!("{}", "Running tests".blue().bold());

        let test_dir = self.build_dir.join("bin").join("tests");
        if !test_dir.exists() {
            println!("Test directory not found: {}", test_dir.display());
            return Ok(());
        }

        let mut failures = 0;
        let mut success = 0;

        for test in &config.targets.test {
            let test_name = if cfg!(target_os = "windows") {
                format!("{}.exe", test.name)
            } else {
                test.name.clone()
            };

            let test_path = test_dir.join(&test_name);

            if !test_path.exists() {
                println!(
                    "{} {}: {}",
                    "SKIP".yellow(),
                    test.name,
                    "Test executable not found"
                );
                continue;
            }

            // 환경 변수 설정: 공유 라이브러리 경로
            let mut cmd = Command::new(&test_path);
            if cfg!(target_os = "linux") {
                let lib_path = self.build_dir.join("lib");
                cmd.env("LD_LIBRARY_PATH", &lib_path);
            } else if cfg!(target_os = "macos") {
                let lib_path = self.build_dir.join("lib");
                cmd.env("DYLD_LIBRARY_PATH", &lib_path);
            } else if cfg!(target_os = "windows") {
                let lib_path = self.build_dir.join("lib");
                cmd.env(
                    "PATH",
                    format!(
                        "{};{}",
                        lib_path.display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                );
            }

            println!("Running test: {}", test.name);

            match cmd.status() {
                Ok(status) => {
                    if status.success() {
                        println!("{} {}", "PASS".green(), test.name);
                        success += 1;
                    } else {
                        println!(
                            "{} {} (code: {})",
                            "FAIL".red(),
                            test.name,
                            status.code().unwrap_or(-1)
                        );
                        failures += 1;
                    }
                }
                Err(e) => {
                    println!("{} {}: {}", "ERROR".red(), test.name, e);
                    failures += 1;
                }
            }
        }

        println!("\nTest Results: {} passed, {} failed", success, failures);

        if failures > 0 {
            return Err(BuildError::CompilerError(format!(
                "{} tests failed",
                failures
            )));
        }

        Ok(())
    }

    fn load_config(&self) -> BuildResult<&BuildConfig> {
        if self.config.is_none() {
            let mut this = self as *const Self as *mut Self;
            // SAFETY: We're temporarily mutating self to cache the config
            unsafe {
                (*this).config = Some(BuildConfig::from_file(&self.project_dir)?);
            }
        }

        Ok(self.config.as_ref().unwrap())
    }
}
