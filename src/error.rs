use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BuildError {
    #[error("IO 오류: {0}")]
    IoError(#[from] std::io::Error),

    #[error("설정 파싱 오류: {0}")]
    ConfigParsingError(String),

    #[error("컴파일러 오류: {0}")]
    CompilerError(String),

    #[error("링커 오류: {0}")]
    LinkerError(String),

    #[error("의존성 오류: {0}")]
    DependencyError(String),

    #[error("경로 오류: {0}")]
    PathError(String),

    #[error("컴파일러 {0}이(가) 설치되지 않았습니다")]
    CompilerNotFound(String),

    #[error("프로젝트 설정 파일 {0}을(를) 찾을 수 없습니다")]
    ConfigNotFound(PathBuf),

    #[error("타겟 {0}에 대한 소스 파일을 찾을 수 없습니다")]
    NoSourceFiles(String),

    #[error("실행 파일 {0}을(를) 찾을 수 없습니다")]
    ExecutableNotFound(PathBuf),
}

pub type BuildResult<T> = Result<T, BuildError>;
