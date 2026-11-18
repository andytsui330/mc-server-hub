//! 系統差異：只有這一區看作業系統，其他程式一律呼叫這裡（沿用 1.x 的原則）。
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Win,
    Mac,
    Other,
}

pub const OS: Os = if cfg!(windows) {
    Os::Win
} else if cfg!(target_os = "macos") {
    Os::Mac
} else {
    Os::Other
};

/// 設定檔放固定位置；Windows 用 %APPDATA%，沒有就退回使用者資料夾。
pub fn data_dir(os: Os, appdata: Option<&Path>, home: &Path) -> PathBuf {
    match os {
        Os::Win => appdata.unwrap_or(home).join("MCServerHub"),
        Os::Mac => home.join("Library/Application Support/MCServerHub"),
        Os::Other => home.join(".config/MCServerHub"),
    }
}

pub fn start_script_name(os: Os) -> &'static str {
    if os == Os::Win { "start.bat" } else { "start.sh" }
}

/// Forge／NeoForge 安裝後的參數檔。
pub fn args_file_name(os: Os) -> &'static str {
    if os == Os::Win { "win_args.txt" } else { "unix_args.txt" }
}

/// 平台自己產生的啟動檔內容。Windows：CRLF＋系統編碼（記事本存的中文 bat 多半是 Big5）；Mac：LF＋UTF-8。
pub fn start_script(java: &str, args: &str, os: Os) -> crate::Result<Vec<u8>> {
    if os == Os::Win {
        let text = format!("@echo off\r\ncd /d \"%~dp0\"\r\n\"{java}\" {args}\r\npause\r\n");
        return encode_text(&text).ok_or_else(|| {
            crate::UserError::new("Java 或伺服器的路徑裡有這台電腦的系統編碼存不了的字，請把資料夾改成英文或中文名稱再試。")
        });
    }
    Ok(format!("#!/bin/sh\ncd \"$(dirname \"$0\")\"\nexec \"{java}\" {args}\n").into_bytes())
}

/// 寫啟動檔；Mac 加上可執行權限。回傳檔名。
pub fn write_start_script(folder: &Path, java: &str, args: &str) -> crate::Result<&'static str> {
    let name = start_script_name(OS);
    let path = folder.join(name);
    crate::fs_safe::write_atomic(&path, &start_script(java, args, OS)?)
        .map_err(|e| crate::UserError::new(format!("寫不進啟動檔 {name}：{e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(name)
}

pub fn java_help_url(need: Option<u32>, os: Os, arch: &str) -> String {
    let need = need.unwrap_or(21);
    if os == Os::Win {
        format!("https://adoptium.net/temurin/releases/?version={need}&os=windows&arch=x64")
    } else {
        format!("https://adoptium.net/temurin/releases/?version={need}&os=mac&arch={arch}")
    }
}

/// Apple 晶片上跑 Intel 版程式（Rosetta）時編譯目標會說 x86_64，要另外問系統。
pub fn mac_arch() -> &'static str {
    let translated = run_with_timeout(Command::new("sysctl").args(["-n", "sysctl.proc_translated"]), Duration::from_secs(5))
        .is_some_and(|o| o.stdout.trim_ascii() == b"1");
    if translated || cfg!(target_arch = "aarch64") { "aarch64" } else { "x64" }
}

/// 讀文字檔內容：先試 UTF-8（含 BOM），不行就用系統編碼，壞字元換成替代字。
pub fn decode_text(data: &[u8]) -> String {
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    match std::str::from_utf8(data) {
        Ok(s) => s.to_string(),
        Err(_) => ansi::decode(data),
    }
}

/// 轉成系統編碼；有存不了的字回傳 None（不默默換成問號，免得啟動檔路徑壞掉）。
pub fn encode_text(text: &str) -> Option<Vec<u8>> {
    ansi::encode(text)
}

/// 子程序：不跳黑窗、不接鍵盤輸入。
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// 用檔案總管／Finder 打開資料夾（1.x 的 open_path），不等它關。
pub fn open_path(path: &std::path::Path) -> std::io::Result<()> {
    command(if OS == Os::Win { "explorer" } else { "open" }).arg(path).spawn().map(drop)
}

/// 本地時間「年月日_時分秒」，用在備份資料夾與安裝紀錄的檔名（1.x 的 time.strftime）。
pub fn local_stamp() -> String {
    #[cfg(windows)]
    {
        let mut t = unsafe { std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>() };
        unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
        format!("{:04}{:02}{:02}_{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
    }
    #[cfg(not(windows))]
    {
        run_with_timeout(command("date").arg("+%Y%m%d_%H%M%S"), Duration::from_secs(5))
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| s.len() == 15)
            .unwrap_or_else(|| format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())))
    }
}

/// 跑完拿輸出；超時就結束它並回傳 None。
/// shortcut: 輸出超過管線緩衝（數 KB）會卡到逾時, 之後要跑大量輸出的程式時改成另開執行緒讀
pub fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Option<Output> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(windows)]
mod ansi {
    use windows_sys::Win32::Globalization::{CP_ACP, GetACP, MultiByteToWideChar, WC_NO_BEST_FIT_CHARS, WideCharToMultiByte};

    pub fn decode(data: &[u8]) -> String {
        if data.is_empty() {
            return String::new();
        }
        unsafe {
            let n = MultiByteToWideChar(CP_ACP, 0, data.as_ptr(), data.len() as i32, std::ptr::null_mut(), 0);
            let mut wide = vec![0u16; n.max(0) as usize];
            let n = MultiByteToWideChar(CP_ACP, 0, data.as_ptr(), data.len() as i32, wide.as_mut_ptr(), n);
            String::from_utf16_lossy(&wide[..n.max(0) as usize])
        }
    }

    pub fn encode(text: &str) -> Option<Vec<u8>> {
        // 系統若設成「使用 UTF-8 全球語言支援」，系統編碼就是 UTF-8，不能帶下面的旗標
        if text.is_empty() || unsafe { GetACP() } == 65001 {
            return Some(text.as_bytes().to_vec());
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let mut lossy = 0;
        unsafe {
            let n = WideCharToMultiByte(CP_ACP, WC_NO_BEST_FIT_CHARS, wide.as_ptr(), wide.len() as i32,
                std::ptr::null_mut(), 0, std::ptr::null(), &mut lossy);
            let mut out = vec![0u8; n.max(0) as usize];
            let n = WideCharToMultiByte(CP_ACP, WC_NO_BEST_FIT_CHARS, wide.as_ptr(), wide.len() as i32,
                out.as_mut_ptr(), n, std::ptr::null(), &mut lossy);
            (n > 0 && lossy == 0).then(|| {
                out.truncate(n as usize);
                out
            })
        }
    }
}

#[cfg(not(windows))]
mod ansi {
    pub fn decode(data: &[u8]) -> String {
        String::from_utf8_lossy(data).into_owned()
    }

    pub fn encode(text: &str) -> Option<Vec<u8>> {
        Some(text.as_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_per_os() {
        let home = Path::new("/h");
        assert_eq!(data_dir(Os::Win, Some(Path::new("/ad")), home), Path::new("/ad/MCServerHub"));
        assert_eq!(data_dir(Os::Win, None, home), Path::new("/h/MCServerHub"));
        assert_eq!(data_dir(Os::Mac, None, home), Path::new("/h/Library/Application Support/MCServerHub"));
    }

    #[test]
    fn text_round_trip() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFjava -jar"), "java -jar");
        let zh = "C:\\伺服器\\java.exe";
        assert_eq!(decode_text(&encode_text(zh).unwrap()), zh);
        if cfg!(windows) && unsafe_acp_is_legacy() {
            assert!(encode_text("😀").is_none(), "系統編碼存不了的字要擋下");
        }
    }

    #[cfg(windows)]
    fn unsafe_acp_is_legacy() -> bool {
        unsafe { windows_sys::Win32::Globalization::GetACP() != 65001 }
    }
    #[cfg(not(windows))]
    fn unsafe_acp_is_legacy() -> bool {
        false
    }

    #[test]
    fn timeout_kills() {
        let mut cmd = if cfg!(windows) { command("ping") } else { command("sleep") };
        if cfg!(windows) { cmd.args(["-n", "5", "127.0.0.1"]) } else { cmd.arg("5") };
        let t = Instant::now();
        assert!(run_with_timeout(&mut cmd, Duration::from_millis(300)).is_none());
        assert!(t.elapsed() < Duration::from_secs(3));
    }
}
