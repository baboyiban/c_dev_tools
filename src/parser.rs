use crate::error::{BuildError, BuildResult};
use lazy_static::lazy_static;
use log::{debug, warn};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct DependencyParser {
    source_extensions: HashSet<String>,
    header_extensions: HashSet<String>,
}

impl DependencyParser {
    pub fn new() -> Self {
        let mut source_extensions = HashSet::new();
        source_extensions.insert("c".to_string());
        source_extensions.insert("cpp".to_string());
        source_extensions.insert("cxx".to_string());
        source_extensions.insert("cc".to_string());

        let mut header_extensions = HashSet::new();
        header_extensions.insert("h".to_string());
        header_extensions.insert("hpp".to_string());
        header_extensions.insert("hxx".to_string());
        header_extensions.insert("hh".to_string());

        DependencyParser {
            source_extensions,
            header_extensions,
        }
    }

    pub fn parse_dependencies(
        &self,
        source_file: &Path,
        include_dirs: &[PathBuf],
    ) -> BuildResult<HashSet<PathBuf>> {
        let mut dependencies = HashSet::new();
        let content = std::fs::read_to_string(source_file).map_err(|e| BuildError::IoError(e))?;

        // 직접 포함되는 헤더 파일 추출
        let direct_includes = self.extract_includes(&content);

        // 헤더 파일 경로 해결 및 재귀적 의존성 검색
        for include in direct_includes {
            if let Some(header_path) = self.resolve_header_path(&include, source_file, include_dirs)
            {
                dependencies.insert(header_path.clone());

                // 간접 의존성 추가 (재귀적)
                let indirect_deps = self.parse_dependencies(&header_path, include_dirs)?;
                dependencies.extend(indirect_deps);
            } else {
                // 표준 라이브러리 헤더일 가능성이 있음
                if !is_likely_standard_header(&include) {
                    warn!(
                        "Could not resolve header: {} included from {}",
                        include,
                        source_file.display()
                    );
                }
            }
        }

        Ok(dependencies)
    }

    fn extract_includes(&self, content: &str) -> HashSet<String> {
        lazy_static! {
            static ref INCLUDE_RE: Regex =
                Regex::new(r#"#\s*include\s+(?:<([^>]+)>|"([^"]+)")"#).unwrap();
        }

        let mut includes = HashSet::new();

        for cap in INCLUDE_RE.captures_iter(content) {
            if let Some(system_include) = cap.get(1) {
                includes.insert(system_include.as_str().to_string());
            } else if let Some(local_include) = cap.get(2) {
                includes.insert(local_include.as_str().to_string());
            }
        }

        includes
    }

    fn resolve_header_path(
        &self,
        include_name: &str,
        source_file: &Path,
        include_dirs: &[PathBuf],
    ) -> Option<PathBuf> {
        // 소스 파일과 동일한 디렉토리에서 상대 경로 확인
        if let Some(parent) = source_file.parent() {
            let relative_path = parent.join(include_name);
            if relative_path.exists() {
                return Some(relative_path);
            }
        }

        // 포함 디렉토리 검색
        for dir in include_dirs {
            let header_path = dir.join(include_name);
            if header_path.exists() {
                return Some(header_path);
            }
        }

        None
    }

    pub fn build_dependency_graph(
        &self,
        source_files: &[PathBuf],
        include_dirs: &[PathBuf],
    ) -> BuildResult<HashMap<PathBuf, HashSet<PathBuf>>> {
        let mut dependency_graph: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();

        for source_file in source_files {
            let dependencies = self.parse_dependencies(source_file, include_dirs)?;
            dependency_graph.insert(source_file.clone(), dependencies);
        }

        Ok(dependency_graph)
    }
}

fn is_likely_standard_header(header_name: &str) -> bool {
    // C 표준 라이브러리 헤더
    const C_STD_HEADERS: &[&str] = &[
        "assert.h",
        "complex.h",
        "ctype.h",
        "errno.h",
        "fenv.h",
        "float.h",
        "inttypes.h",
        "iso646.h",
        "limits.h",
        "locale.h",
        "math.h",
        "setjmp.h",
        "signal.h",
        "stdalign.h",
        "stdarg.h",
        "stdatomic.h",
        "stdbool.h",
        "stddef.h",
        "stdint.h",
        "stdio.h",
        "stdlib.h",
        "stdnoreturn.h",
        "string.h",
        "tgmath.h",
        "threads.h",
        "time.h",
        "uchar.h",
        "wchar.h",
        "wctype.h",
    ];

    // C++ 표준 라이브러리 헤더
    const CPP_STD_HEADERS: &[&str] = &[
        "algorithm",
        "any",
        "array",
        "atomic",
        "bitset",
        "cassert",
        "ccomplex",
        "cctype",
        "cerrno",
        "cfenv",
        "cfloat",
        "charconv",
        "chrono",
        "cinttypes",
        "ciso646",
        "climits",
        "clocale",
        "cmath",
        "codecvt",
        "complex",
        "condition_variable",
        "csetjmp",
        "csignal",
        "cstdalign",
        "cstdarg",
        "cstdbool",
        "cstddef",
        "cstdint",
        "cstdio",
        "cstdlib",
        "cstring",
        "ctgmath",
        "ctime",
        "cuchar",
        "cwchar",
        "cwctype",
        "deque",
        "exception",
        "execution",
        "filesystem",
        "forward_list",
        "fstream",
        "functional",
        "future",
        "initializer_list",
        "iomanip",
        "ios",
        "iosfwd",
        "iostream",
        "istream",
        "iterator",
        "limits",
        "list",
        "locale",
        "map",
        "memory",
        "memory_resource",
        "mutex",
        "new",
        "numeric",
        "optional",
        "ostream",
        "queue",
        "random",
        "ratio",
        "regex",
        "scoped_allocator",
        "set",
        "shared_mutex",
        "sstream",
        "stack",
        "stdexcept",
        "streambuf",
        "string",
        "string_view",
        "system_error",
        "thread",
        "tuple",
        "type_traits",
        "typeindex",
        "typeinfo",
        "unordered_map",
        "unordered_set",
        "utility",
        "valarray",
        "variant",
        "vector",
    ];

    C_STD_HEADERS.contains(&header_name) || CPP_STD_HEADERS.contains(&header_name)
}
