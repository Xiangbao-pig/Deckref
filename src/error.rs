//! 统一错误类型，禁止 unwrap，所有错误必须在此处理或传播。

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("环境变量错误: {0}")]
    EnvVar(#[from] std::env::VarError),

    #[error("通知/监听错误: {0}")]
    Notify(#[from] notify::Error),

    #[error("图片解码/编码错误: {0}")]
    Image(#[from] image::ImageError),

    #[error("其他: {0}")]
    Other(String),
}

pub type AppResult<T> = Result<T, AppError>;
