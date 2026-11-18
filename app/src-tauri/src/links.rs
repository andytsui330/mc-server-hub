pub fn checked_url(input: &str) -> Result<String, String> {
    let url = tauri::Url::parse(input).map_err(|_| "網址格式不正確")?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || input.chars().any(char::is_control) {
        return Err("只允許開啟 HTTPS 網址".into());
    }
    Ok(url.to_string())
}

pub fn open(input: &str) -> Result<(), String> {
    let url = checked_url(input)?;
    let mut cmd = if cfg!(windows) {
        let mut cmd = mc_hub_core::platform::command("rundll32.exe");
        cmd.arg("url.dll,FileProtocolHandler");
        cmd
    } else {
        mc_hub_core::platform::command("open")
    };
    cmd.arg(url).spawn().map(drop).map_err(|e| format!("打不開網址：{e}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn external_links_only_accept_https() {
        assert!(super::checked_url("https://example.com/docs?q=繁中").is_ok());
        for url in ["file:///C:/Windows", "javascript:alert(1)", "http://example.com", "https://", "https://user:secret@example.com", "https://example.com\n"] {
            assert!(super::checked_url(url).is_err(), "不該接受：{url:?}");
        }
    }
}

