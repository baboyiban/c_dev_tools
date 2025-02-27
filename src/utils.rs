use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// 파일 확장자 가져오기
pub fn get_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|s| s.to_lowercase())
}

/// 소스 파일인지 확인
pub fn is_source_file(path: &Path) -> bool {
    if let Some(ext) = get_extension(path) {
        matches!(ext.as_str(), "c" | "cpp" | "cxx" | "cc")
    } else {
        false
    }
}

/// 헤더 파일인지 확인
pub fn is_header_file(path: &Path) -> bool {
    if let Some(ext) = get_extension(path) {
        matches!(ext.as_str(), "h" | "hpp" | "hxx" | "hh")
    } else {
        false
    }
}

/// 파일 프리픽스(확장자 제외) 가져오기
pub fn get_file_prefix(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(OsStr::to_str)
        .map(|s| s.to_string())
}

/// 파일 이름 가져오기
pub fn get_file_name(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(OsStr::to_str)
        .map(|s| s.to_string())
}

/// 경로를 상대 경로로 변환
pub fn to_relative_path(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() {
        if let Ok(rel_path) = path.strip_prefix(base) {
            return rel_path.to_path_buf();
        }
    }
    path.to_path_buf()
}

/// 경로를 절대 경로로 변환
pub fn to_absolute_path(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// 중복 경로 제거
pub fn deduplicate_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for path in paths {
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => path.clone(),
        };

        if seen.insert(canonical.clone()) {
            result.push(path.clone());
        }
    }

    result
}

/// 타임스탬프 비교 (파일이 더 최신인지 확인)
pub fn is_newer_than(path1: &Path, path2: &Path) -> bool {
    let metadata1 = match std::fs::metadata(path1) {
        Ok(m) => m,
        Err(_) => return true, // 파일이 없으면 최신으로 간주
    };

    let metadata2 = match std::fs::metadata(path2) {
        Ok(m) => m,
        Err(_) => return true, // 대상 파일이 없으면 최신으로 간주
    };

    let time1 = metadata1
        .modified()
        .unwrap_or_else(|_| std::time::SystemTime::now());
    let time2 = metadata2
        .modified()
        .unwrap_or_else(|_| std::time::SystemTime::UNIX_EPOCH);

    time1 > time2
}

/// 파일 이름을 기반으로 출력 경로 생성
pub fn derive_output_path(
    source_path: &Path,
    base_dir: &Path,
    output_dir: &Path,
    extension: &str,
) -> PathBuf {
    let rel_path = to_relative_path(source_path, base_dir);
    let file_prefix = get_file_prefix(&rel_path).unwrap_or_else(|| "output".to_string());

    let mut output_path = output_dir.to_path_buf();
    if let Some(parent) = rel_path.parent() {
        output_path = output_path.join(parent);
    }

    output_path.join(format!("{}.{}", file_prefix, extension))
}

/// 디렉토리 내의 모든 파일을 재귀적으로 수집
pub fn collect_files_with_extension(dir: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut result = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                // 재귀적으로 하위 디렉토리 처리
                let mut sub_files = collect_files_with_extension(&path, extensions);
                result.append(&mut sub_files);
            } else if let Some(ext) = get_extension(&path) {
                // 확장자 확인
                if extensions.contains(&ext.as_str()) {
                    result.push(path);
                }
            }
        }
    }

    result
}

/// 파일의 내용 해시 계산
pub fn hash_file_content(path: &Path) -> Result<String, std::io::Error> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;

    let mut hasher = Sha256::new();
    hasher.update(&content);
    let result = hasher.finalize();

    Ok(format!("{:x}", result))
}

/// 명령어 실행 결과를 문자열로 반환
pub fn execute_command_and_capture_output(
    command: &str,
    args: &[&str],
    working_dir: Option<&Path>,
) -> Result<String, std::io::Error> {
    let mut cmd = std::process::Command::new(command);
    cmd.args(args);

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    let output = cmd.output()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Command failed: {}", stderr),
        ))
    }
}

/// 대규모 C 프로젝트에서 일반적으로 사용되는 파일 확장자 목록
pub fn get_common_source_extensions() -> Vec<&'static str> {
    vec!["c", "cpp", "cxx", "cc"]
}

pub fn get_common_header_extensions() -> Vec<&'static str> {
    vec!["h", "hpp", "hxx", "hh"]
}

/// 파일이 존재하는지 확인하고 없으면 생성
pub fn ensure_file_exists(path: &Path, default_content: &str) -> Result<(), std::io::Error> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, default_content)?;
    }
    Ok(())
}

/// 디렉토리가 존재하는지 확인하고 없으면 생성
pub fn ensure_directory_exists(path: &Path) -> Result<(), std::io::Error> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

/// 도구가 시스템에 설치되어 있는지 확인
pub fn is_tool_installed(tool: &str) -> bool {
    which::which(tool).is_ok()
}

/// 컴파일러 버전 정보 가져오기
pub fn get_compiler_version(compiler: &str) -> Option<String> {
    match execute_command_and_capture_output(compiler, &["--version"], None) {
        Ok(output) => {
            let first_line = output.lines().next()?;
            Some(first_line.trim().to_string())
        }
        Err(_) => None,
    }
}

/// 파일 수정 시간 가져오기
pub fn get_file_modification_time(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// 파일을 새로운 위치로 복사, 필요한 디렉토리 생성
pub fn copy_file_with_dirs(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src, dst)?;
    Ok(())
}

/// 플랫폼별 라이브러리 확장자 가져오기
pub fn get_platform_lib_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "dll"
    } else if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    }
}

/// 플랫폼별 실행 파일 확장자 가져오기
pub fn get_platform_exe_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "exe"
    } else {
        ""
    }
}

/// 플랫폼별 정적 라이브러리 확장자 가져오기
pub fn get_platform_static_lib_extension() -> &'static str {
    if cfg!(target_os = "windows") {
        "lib"
    } else {
        "a"
    }
}

/// 플랫폼별 공유 라이브러리 prefix 가져오기
pub fn get_platform_lib_prefix() -> &'static str {
    if cfg!(target_os = "windows") {
        ""
    } else {
        "lib"
    }
}

/// 디버깅 출력용 특정 길이의 줄 구분자
pub fn get_separator(length: usize) -> String {
    "=".repeat(length)
}
