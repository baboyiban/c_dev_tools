mod builder;
mod config;
mod dependency;
mod error;
mod parser;
mod runner;
mod utils;

use clap::{Parser, Subcommand};
use colored::Colorize;
use log::{error, info};
use std::path::PathBuf;

use crate::builder::Builder;
use crate::config::BuildConfig;
use crate::dependency::DependencyManager;
use crate::runner::Runner;

/// 대규모 C 프로젝트 빌드 시스템
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 프로젝트 초기화
    Init {
        /// 프로젝트 디렉토리 (기본: 현재 디렉토리)
        #[arg(short, long)]
        directory: Option<PathBuf>,
    },
    /// 전체 프로젝트 빌드
    Build {
        /// 프로젝트 디렉토리 (기본: 현재 디렉토리)
        #[arg(short, long)]
        directory: Option<PathBuf>,

        /// 빌드 구성 (debug/release)
        #[arg(short, long, default_value = "debug")]
        configuration: String,

        /// 빌드 작업 병렬 처리를 위한 스레드 수 (기본: 시스템 CPU 코어 수)
        #[arg(short, long)]
        jobs: Option<usize>,

        /// 변경된 파일만 다시 빌드
        #[arg(short, long)]
        incremental: bool,

        /// 빌드 후 자동으로 실행
        #[arg(short, long)]
        run: bool,

        /// 자세한 빌드 정보 출력
        #[arg(short, long)]
        verbose: bool,
    },
    /// 프로젝트 정리 (빌드 결과물 삭제)
    Clean {
        /// 프로젝트 디렉토리 (기본: 현재 디렉토리)
        #[arg(short, long)]
        directory: Option<PathBuf>,
    },
    /// 프로젝트 실행
    Run {
        /// 프로젝트 디렉토리 (기본: 현재 디렉토리)
        #[arg(short, long)]
        directory: Option<PathBuf>,

        /// 실행 인자
        #[arg(short, long)]
        args: Option<String>,
    },
    /// 의존성 다운로드 및 설치
    Dependencies {
        /// 프로젝트 디렉토리 (기본: 현재 디렉토리)
        #[arg(short, long)]
        directory: Option<PathBuf>,

        /// 의존성 업데이트
        #[arg(short, long)]
        update: bool,
    },
}

fn main() {
    env_logger::init();
    let cli = Cli::parse();

    let current_dir = std::env::current_dir().expect("현재 디렉토리를 확인할 수 없습니다");

    match cli.command {
        Command::Init { directory } => {
            let project_dir = directory.unwrap_or(current_dir);
            init_project(&project_dir);
        }
        Command::Build {
            directory,
            configuration,
            jobs,
            incremental,
            run,
            verbose,
        } => {
            let project_dir = directory.unwrap_or(current_dir);
            let jobs = jobs.unwrap_or_else(|| num_cpus::get());

            let mut builder = Builder::new(&project_dir, &configuration, jobs);
            builder.set_incremental(incremental);
            builder.set_verbose(verbose);

            if let Err(e) = builder.build() {
                error!("빌드 실패: {}", e);
                std::process::exit(1);
            }

            if run {
                let runner = Runner::new(&project_dir);
                if let Err(e) = runner.run(None) {
                    error!("실행 실패: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Command::Clean { directory } => {
            let project_dir = directory.unwrap_or(current_dir);
            let builder = Builder::new(&project_dir, "debug", 1);

            if let Err(e) = builder.clean() {
                error!("정리 실패: {}", e);
                std::process::exit(1);
            }
        }
        Command::Run { directory, args } => {
            let project_dir = directory.unwrap_or(current_dir);
            let runner = Runner::new(&project_dir);

            if let Err(e) = runner.run(args.as_deref()) {
                error!("실행 실패: {}", e);
                std::process::exit(1);
            }
        }
        Command::Dependencies { directory, update } => {
            let project_dir = directory.unwrap_or(current_dir);
            let mut dep_manager = DependencyManager::new(&project_dir);

            if update {
                if let Err(e) = dep_manager.update() {
                    error!("의존성 업데이트 실패: {}", e);
                    std::process::exit(1);
                }
            } else {
                if let Err(e) = dep_manager.install() {
                    error!("의존성 설치 실패: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}

fn init_project(directory: &PathBuf) {
    info!("프로젝트 초기화 중: {}", directory.display());

    // 기본 디렉토리 구조 생성
    let dirs = ["src", "include", "lib", "build", "test", "docs", "deps"];

    for dir in dirs {
        let path = directory.join(dir);
        if !path.exists() {
            info!("디렉토리 생성: {}", path.display());
            if let Err(e) = std::fs::create_dir_all(&path) {
                error!("디렉토리 생성 실패 {}: {}", path.display(), e);
            }
        }
    }

    // 빌드 설정 파일 생성
    let config_path = directory.join("cbuild.toml");
    if !config_path.exists() {
        info!("빌드 설정 파일 생성: {}", config_path.display());
        let config_content = r#"# C 프로젝트 빌드 설정
[project]
name = "my_c_project"
version = "0.1.0"
authors = ["Your Name <your.email@example.com>"]
description = "A C project"

[build]
compiler = "gcc"
c_standard = "c11"
cpp_standard = "c++17"
optimization_level = 2 # O2
debug_info = true
warnings_as_errors = false

[dependencies]
# 예시: 의존성 설정
# libcurl = { version = "7.75.0", features = ["ssl"] }

[targets]
# 메인 실행 파일
[[targets.executable]]
name = "main"
src = ["src/main.c"]
include_dirs = ["include"]
link_dirs = ["lib"]
libs = []

# 정적 라이브러리 예시
# [[targets.static_lib]]
# name = "mylib"
# src = ["src/lib/*.c"]
# include_dirs = ["include"]

# 테스트 실행 파일
# [[targets.test]]
# name = "test_all"
# src = ["test/test_*.c"]
# include_dirs = ["include", "test/include"]
# link_dirs = ["lib"]
# libs = ["mylib"]
"#;
        if let Err(e) = std::fs::write(&config_path, config_content) {
            error!("설정 파일 생성 실패: {}", e);
        }
    }

    // 기본 소스 파일 생성
    let main_c_path = directory.join("src/main.c");
    if !main_c_path.exists() {
        info!("기본 소스 파일 생성: {}", main_c_path.display());
        let main_c_content = r#"/**
 * @file main.c
 * @brief 프로그램 메인 진입점
 */

#include <stdio.h>
#include <stdlib.h>

/**
 * 프로그램 메인 함수
 */
int main(int argc, char *argv[]) {
    printf("Hello, C Build System!\n");
    return 0;
}
"#;
        if let Err(e) = std::fs::write(&main_c_path, main_c_content) {
            error!("소스 파일 생성 실패: {}", e);
        }
    }

    println!("{}", "프로젝트 초기화 완료!".green());
    println!("다음 단계:");
    println!("  1. cbuild.toml 설정 파일을 프로젝트에 맞게 수정하세요.");
    println!("  2. 소스 코드를 src/ 디렉토리에 추가하세요.");
    println!("  3. 다음 명령으로 프로젝트를 빌드하세요: c_build_system build");
}
