//! MC 開服平台核心：開關機、建服、備份、指令、設定等邏輯都放這裡，不引用任何 Tauri 程式（計畫書第三節原則 0）。
pub mod backup;
pub mod automation;
pub mod commands;
pub mod create;
pub mod fs_safe;
pub mod hub;
pub mod jar;
pub mod java;
pub mod net;
pub mod mods;
pub mod java_install;
pub mod platform;
pub mod players;
pub mod properties;
pub mod server;
pub mod settings;

/// 要直接顯示給使用者看的白話錯誤（1.x 的 UserError）。
#[derive(Debug, Clone, PartialEq)]
pub struct UserError {
    pub msg: String,
    pub help_url: Option<String>,
}

impl UserError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self { msg: msg.into(), help_url: None }
    }
}

impl std::fmt::Display for UserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for UserError {}

pub type Result<T> = std::result::Result<T, UserError>;
