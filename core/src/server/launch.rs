//! 看懂啟動檔（start.bat／run.bat／start.sh）、server.properties、eula.txt，以及開機失敗的白話翻譯。
use crate::platform::{self, OS, Os};
use crate::{Result, UserError};
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

/// mode：Direct＝平台直接跑 java；Script＝看不懂，執行啟動檔本身（1.x 的 "bat"）。
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub cmd: String,
    pub java: Option<String>,
    pub mode: Mode,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Direct,
    Script,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        if self == Mode::Direct { "direct" } else { "bat" }
    }
}

const UTF8_FLAGS: &str = " -Dstdout.encoding=UTF-8 -Dstderr.encoding=UTF-8";
const SKIP_WORDS: [&str; 12] = ["rem", "title", "chcp", "color", "cls", "setlocal", "endlocal", "pause", "timeout", "exit", "mode", "popd"];
const SH_CD_OK: [&str; 7] = [
    r#"cd "$(dirname "$0")""#, r#"cd "$(dirname -- "$0")""#, r#"cd "$(dirname "$0")" || exit 1"#,
    r#"cd "$(dirname "$0")" || exit"#, "cd .", "cd ./", "set -e",
];

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

/// Python 的 str.splitlines()：除了 \n、\r\n、\r，也認 \v \f \x1c-\x1e \x85 \u2028 \u2029。
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            out.push(&text[start..i]);
            start = i + c.len_utf8();
            if c == '\r' && it.peek().is_some_and(|&(_, n)| n == '\n') {
                it.next();
                start += 1;
            }
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// Python 的 s.split(None, 1)：第一個字，和去掉前面空白的其餘部分。
fn split1(s: &str) -> (&str, Option<&str>) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], Some(s[i..].trim_start()).filter(|r| !r.is_empty())),
        None => (s, None),
    }
}

/// 回傳 (執行檔, 指令中執行檔結束的位置)；空字串回 None。
pub(crate) fn first_token(s: &str) -> Option<(&str, usize)> {
    if let Some(rest) = s.strip_prefix('"') {
        return Some(match rest.find('"') {
            Some(end) => (&rest[..end], end + 2),
            None => (rest, s.len()),
        });
    }
    let word = s.split_whitespace().next()?;
    Some((word, s.find(word).unwrap() + word.len()))
}

/// 路徑最後一段（Windows 匯出的標準答案：/ 和 \ 都當分隔）。
fn file_name(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// Python Path.stem：去掉最後一個副檔名（開頭的句點不算）。
fn stem(p: &str) -> &str {
    let name = file_name(p);
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

pub fn read_text_any(path: &Path) -> std::io::Result<String> {
    Ok(platform::decode_text(&std::fs::read(path)?))
}

/// 把 bat 變數換成實際值；有不認識的變數就回 None（交給 bat 自己跑）。
fn expand(s: &str, env: &HashMap<String, String>, server_dir: &Path) -> Option<String> {
    static DP0: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)%~dp0"));
    static ARG: LazyLock<Regex> = LazyLock::new(|| re(r"%[*\d]"));
    static VAR: LazyLock<Regex> = LazyLock::new(|| re(r"%([^%\s]+)%"));
    let dir = format!("{}\\", server_dir.display());
    let s = DP0.replace_all(s, regex::NoExpand(&dir));
    let s = ARG.replace_all(&s, "");
    let mut missing = false;
    let s = VAR.replace_all(&s, |c: &regex::Captures| match env.get(&c[1].to_uppercase()) {
        Some(v) => v.clone(),
        None => {
            missing = true;
            String::new()
        }
    });
    (!missing && !s.contains('%')).then(|| s.into_owned())
}

/// bat 看不懂時，盡量找出它會用哪個 Java，開機前才查得了版本。
fn java_mentioned(text: &str) -> Option<String> {
    static SKIP: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(rem\b|echo\b|::)"));
    static QUOTED: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)"([^"%\r\n]*\\javaw?(?:\.exe)?)""#));
    static BARE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(^|[\s(])javaw?(\.exe)?\s+-"));
    for raw in splitlines(text) {
        let line = raw.trim().trim_start_matches('@').trim();
        if line.is_empty() || SKIP.is_match(line) {
            continue;
        }
        if let Some(m) = QUOTED.captures(line) {
            return Some(m[1].to_string());
        }
        if BARE.is_match(line) {
            return Some("java".into());
        }
    }
    None
}

fn direct(java_line: &str) -> Launch {
    let (exe, end) = first_token(java_line).expect("java 那行不會是空的");
    Launch { cmd: format!("{}{UTF8_FLAGS}{}", &java_line[..end], &java_line[end..]), java: Some(exe.into()), mode: Mode::Direct }
}

/// 照 POSIX sh 的引號規則拆字（呼叫前已擋掉 \ $ ` 等字元，所以只需處理引號）；引號沒關回 None。
pub fn sh_split(line: &str) -> Option<Vec<String>> {
    let (mut out, mut cur, mut quote, mut has) = (Vec::new(), String::new(), None, false);
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => (quote, has) = (Some(c), true),
            (None, c) if c.is_whitespace() => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, c) => (cur.push(c), has = true).1,
        }
    }
    if quote.is_some() {
        return None;
    }
    if has {
        out.push(cur);
    }
    Some(out)
}

/// Mac 的 start.sh／run.sh：只有一行 java（可加 exec、結尾 "$@"）就直接跑；其他寫法交給 sh 執行。
fn parse_launch_sh(text: &str, script: &str) -> Launch {
    static JAVA: LazyLock<Regex> = LazyLock::new(|| re(r#"(?m)^\s*(?:exec\s+)?"?([^\s"]*/java|java)"?\s+"#));
    static EXEC: LazyLock<Regex> = LazyLock::new(|| re(r"^exec\s+"));
    static ARGS: LazyLock<Regex> = LazyLock::new(|| re(r#"\s+"\$@"\s*$"#));
    static BAD: LazyLock<Regex> = LazyLock::new(|| re(r"[$`|<>&;()\\]"));
    let fallback = Launch { cmd: format!("/bin/sh ./{script}"), java: JAVA.captures(text).map(|c| c[1].to_string()), mode: Mode::Script };
    let mut java_line = None;
    for raw in splitlines(text) {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || SH_CD_OK.contains(&line) {
            continue;
        }
        if java_line.is_some() {
            return fallback;
        }
        let line = EXEC.replace(line, "");
        let line = ARGS.replace(&line, "").into_owned();
        if BAD.is_match(&line) {
            return fallback;
        }
        match sh_split(&line) {
            Some(parts) if parts.first().is_some_and(|p| file_name(p) == "java") => java_line = Some(line),
            _ => return fallback,
        }
    }
    java_line.map_or(fallback, |l| direct(&l))
}

/// 看得懂就抽出 java 指令直接跑（多加 UTF-8 參數防亂碼）；有分支、管線等複雜寫法就執行啟動檔本身。
pub fn parse_launch(server_dir: &Path, bat: &str) -> std::io::Result<Launch> {
    let text = read_text_any(&server_dir.join(bat))?;
    if bat.to_lowercase().ends_with(".sh") {
        return Ok(parse_launch_sh(&text, bat));
    }
    let env = std::env::vars().map(|(k, v)| (k.to_uppercase(), v)).collect();
    Ok(parse_bat(&text, bat, server_dir, env))
}

fn parse_bat(text: &str, bat: &str, server_dir: &Path, mut env: HashMap<String, String>) -> Launch {
    // Forge 1.21 的 run.bat：先跑 shim.jar --onlyCheckJava 檢查 Java、失敗就 goto 結尾，再跑真正那行。
    // 平台開機前自己會檢查 Java，所以把這段拿掉，改直接跑那行（格式依 2026-11-13 真實安裝 52.1.20 的產物）。
    // 中間那段是「第一個非空白字不是 ) 的行」（1.x 用 (?![ \t]*\)) 寫，Rust regex 沒有 lookahead）。
    static SHIM: LazyLock<Regex> = LazyLock::new(|| {
        re(r"(?im)^[ \t]*java[ \t]+-jar[ \t]+\S+-shim\.jar[ \t]+--onlyCheckJava[ \t]*\r?\n[ \t]*if[ \t]+%ERRORLEVEL%[ \t]+NEQ[ \t]+0[ \t]+\([ \t]*\r?\n(?:[ \t]*(?:[^ \t)\n][^\n]*)?\n)*?[ \t]*\)[ \t]*\r?$")
    });
    static CARET: LazyLock<Regex> = LazyLock::new(|| re(r"\^[ \t]*\r?\n[ \t]*"));
    static ECHO: LazyLock<Regex> = LazyLock::new(|| re(r"^(echo\b|echo[.:,])"));
    static CD_D: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^/d\s+"));
    static SET_Q: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)^set\s+"([^=]+)=(.*)"\s*$"#));
    static SET: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)^set\s+([^=\s/"][^=]*)=(.*)$"#));
    static QUOTES: LazyLock<Regex> = LazyLock::new(|| re(r#""[^"]*""#));
    let text = SHIM.replace_all(text, "");
    let joined = CARET.replace_all(&text, " ");
    // 要寫 .\：有些環境（NoDefaultCurrentDirectoryInExePath）cmd 不會找目前資料夾。
    let fallback = Launch { cmd: format!("cmd.exe /d /c \".\\{bat}\""), java: java_mentioned(&text), mode: Mode::Script };
    env.insert("CD".into(), server_dir.display().to_string());
    let (mut java_line, mut java_idx) = (None::<String>, 0usize);
    let (mut labels, mut gotos) = (HashMap::new(), Vec::new());
    for (i, raw) in splitlines(&joined).into_iter().enumerate() {
        let line = raw.trim().trim_start_matches('@').trim();
        if line.is_empty() || line.starts_with("::") {
            continue;
        }
        if let Some(label) = line.strip_prefix(':') {
            labels.insert(label.split_whitespace().next().unwrap_or("").to_lowercase(), i);
            continue;
        }
        let (word, rest) = split1(line);
        let word = word.to_lowercase();
        if SKIP_WORDS.contains(&word.as_str()) || ECHO.is_match(&word) {
            continue;
        }
        let rest = if line.contains(' ') { rest.unwrap_or("") } else { "" };
        if matches!(word.as_str(), "cd" | "chdir" | "pushd") {
            let arg = CD_D.replace(rest, "");
            let arg = arg.trim().trim_matches('"').trim_end_matches(['\\', '/']);
            if !matches!(arg.to_lowercase().as_str(), "%~dp0" | "." | "%cd%") {
                return fallback;
            }
            continue;
        }
        if word == "set" {
            let Some(m) = SET_Q.captures(line).or_else(|| SET.captures(line)) else { return fallback };
            let Some(value) = expand(&m[2], &env, server_dir) else { return fallback };
            env.insert(m[1].trim().to_uppercase(), value);
            continue;
        }
        if word == "goto" {
            gotos.push((i, rest.trim().trim_start_matches(':').to_lowercase()));
            continue;
        }
        let Some(expanded) = expand(line, &env, server_dir) else { return fallback };
        let Some((exe, _)) = first_token(&expanded) else { return fallback };
        if !matches!(stem(exe).to_lowercase().as_str(), "java" | "javaw") || java_line.is_some() {
            return fallback;
        }
        if QUOTES.replace_all(&expanded, "").contains(['|', '<', '>', '&']) {
            return fallback;
        }
        (java_line, java_idx) = (Some(expanded), i);
    }
    let Some(java_line) = java_line else { return fallback };
    // 自動重開的迴圈（java 之後 goto 回 java 之前）平台不需要；其他 goto 都交給 bat 自己跑。
    if gotos.iter().any(|(idx, target)| *idx < java_idx || labels.get(target).copied().unwrap_or(java_idx + 1) > java_idx) {
        return fallback;
    }
    direct(&java_line)
}

/// 從安裝器產生的 run.bat／run.sh 抽出 `@user_jvm_args.txt @libraries/…args.txt`；找不到回 None。
pub fn modloader_args(text: &str) -> Option<String> {
    static SKIP: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(rem\b|::|#)"));
    static ARG: LazyLock<Regex> = LazyLock::new(|| re(r"^@(?:user_jvm_args\.txt|libraries/[\w./+-]+_args\.txt)$"));
    for raw in splitlines(text) {
        let line = raw.trim();
        if SKIP.is_match(line) || !line.contains("@user_jvm_args.txt") {
            continue;
        }
        // 1.x 用 (?<!\S)…(?!\S) 前後不能接非空白字＝整個字要符合
        let args: Vec<&str> = line.split_whitespace().filter(|w| ARG.is_match(w)).collect();
        if args.len() >= 2 {
            return Some(args.join(" "));
        }
    }
    None
}

pub fn read_prop(server_dir: &Path, key: &str) -> Option<String> {
    crate::properties::effective_value(server_dir,key)
}

pub fn read_port(server_dir: &Path, fallback: u16) -> u16 {
    read_prop(server_dir, "server-port").and_then(|p| p.parse().ok()).unwrap_or(fallback)
}

pub fn port_free(port: u16) -> bool {
    // Windows 有時允許 wildcard 與特定位址重疊綁定；先確認本機沒有服務回應。
    std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127,0,0,1],port)),std::time::Duration::from_millis(100)).is_err()
        && std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
}

pub fn eula_agreed(server_dir: &Path) -> bool {
    static RE: LazyLock<Regex> = LazyLock::new(|| re(r"(?im)^\s*eula\s*=\s*true\s*$"));
    read_text_any(&server_dir.join("eula.txt")).is_ok_and(|t| RE.is_match(&t))
}

pub const EULA_URL: &str = "https://aka.ms/MinecraftEULA";

pub fn write_eula(server_dir: &Path) -> std::io::Result<()> {
    crate::fs_safe::write_atomic(&server_dir.join("eula.txt"), format!("# {EULA_URL}\n# Agreed by the user in MC Server Hub.\neula=true\n").as_bytes())
}

/// 找啟動檔：Windows 找 .bat，Mac 找 .sh；沒有標準檔名時，資料夾裡剛好一個含 java 的才算。
pub fn find_bat(dir: &Path, os: Os) -> Option<String> {
    let name = platform::start_script_name(os);
    if dir.join(name).is_file() {
        return Some(name.into());
    }
    let ext = if os == Os::Win { "bat" } else { "sh" };
    let with_java: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) && p.is_file())
        .filter(|p| read_text_any(p).is_ok_and(|t| t.to_lowercase().contains("java")))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    (with_java.len() == 1).then(|| with_java.into_iter().next().unwrap())
}

/// 把常見的開機失敗翻成白話；正常關機回 None。
pub fn explain_failure(lines: &[String], code: i64, saw_done: bool) -> Option<String> {
    type Rule = (Regex, fn(&regex::Captures) -> String);
    static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
        vec![
            (re(r"UnsupportedClassVersionError.*?class file version (\d+)"), |m| {
                format!("Java 版本太舊：這台伺服器需要 Java {} 以上，請安裝後再開機。", m[1].parse::<i64>().unwrap_or(0) - 44)
            }),
            (re(r"Unable to access jarfile"), |_| {
                format!("找不到伺服器主程式（jar 檔）。請確認 {} 寫的檔名和資料夾裡的檔案一樣。", platform::start_script_name(OS))
            }),
            (re(r"You need to agree to the EULA"), |_| "還沒同意 Minecraft 授權合約（EULA）。再按一次開機就會跳出同意畫面。".into()),
            (re(r"FAILED TO BIND TO PORT|Address already in use"), |_| {
                "連接埠被別的程式佔用了。請關掉另一台伺服器，或改 server.properties 的 server-port。".into()
            }),
            (re(r"Could not reserve enough space|Invalid (?:maximum|initial) heap size|Initial heap size set to a larger value|insufficient memory"), |_| {
                format!("記憶體不夠：{} 的 -Xmx 設得比電腦能給的還多。請改小一點，或先關掉其他程式。", platform::start_script_name(OS))
            }),
            (re(r"'[^'\n]*java[^'\n]*'\s*(?:is not recognized|不是內部或外部命令)|java: (?:command )?not found|No Java runtime present|Unable to locate a Java Runtime"), |_| {
                format!("找不到 Java。請先安裝 Java，或確認 {} 裡的路徑沒打錯。", platform::start_script_name(OS))
            }),
            (re(r"Incompatible mods found|Some of your mods are incompatible|Mod resolution encountered"), |_| {
                "模組有問題：有模組缺少必要的前置模組，或和這個遊戲版本不合。請看主控台最後幾行寫的是哪一個模組。".into()
            }),
            // Forge／NeoForge（FML）缺前置模組或版本不合時的訊息
            (re(r"Missing or unsupported mandatory dependencies"), |_| {
                "模組少了前置模組，或前置模組版本不合（Forge／NeoForge）。主控台會寫哪個模組需要什麼，請到該模組的下載頁補上對應版本，放進 mods 資料夾。".into()
            }),
        ]
    });
    let text = lines[lines.len().saturating_sub(200)..].join("\n");
    if let Some(msg) = RULES.iter().find_map(|(r, f)| r.captures(&text).map(|m| f(&m))) {
        return Some(msg);
    }
    if saw_done {
        return (code != 0).then(|| format!("伺服器意外關閉（結束代碼 {code}）。請看主控台最後幾行，或資料夾裡的 crash-reports。"));
    }
    Some(format!("伺服器還沒開好就結束了（結束代碼 {code}）。請看下方主控台最後幾行的錯誤訊息。"))
}

fn editor() -> &'static str {
    if OS == Os::Win { "記事本" } else { "「文字編輯」" }
}

/// 改啟動檔裡唯一一個 -Xmx（逐位元組改，不動原本編碼）；-Xms 比新值大就一起降。改之前留 .bak。
pub fn set_ram(script: &Path, gb: u32) -> Result<()> {
    static XMX: LazyLock<regex::bytes::Regex> = LazyLock::new(|| regex::bytes::Regex::new(r"-Xmx\d+[GgMm]\b").unwrap());
    static XMS: LazyLock<regex::bytes::Regex> = LazyLock::new(|| regex::bytes::Regex::new(r"-Xms(\d+)([GgMm])\b").unwrap());
    let io = |e: std::io::Error| UserError::new(format!("啟動檔寫不進去：{e}"));
    let data = std::fs::read(script).map_err(io)?;
    if XMX.find_iter(&data).count() != 1 {
        return Err(UserError::new(format!("這份啟動檔不是平台產生的格式（找不到剛好一個 -Xmx），請用{}手動改。", editor())));
    }
    let new = format!("-Xmx{gb}G");
    let mut data = XMX.replace(&data, regex::bytes::NoExpand(new.as_bytes())).into_owned();
    if let Some(m) = XMS.captures(&data) {
        let n: u64 = std::str::from_utf8(&m[1]).ok().and_then(|s| s.parse().ok()).unwrap_or(u64::MAX);
        let mb = n.saturating_mul(if m[2].eq_ignore_ascii_case(b"g") { 1024 } else { 1 });
        if mb > u64::from(gb) * 1024 {
            let range = m.get(0).unwrap().range();
            data.splice(range, format!("-Xms{gb}G").into_bytes());
        }
    }
    let mut bak = script.as_os_str().to_owned();
    bak.push(".bak");
    std::fs::copy(script, bak).map_err(io)?;
    let perms = std::fs::metadata(script).map_err(io)?.permissions(); // Mac 的 start.sh 要保留「可執行」權限
    crate::fs_safe::write_atomic(script, &data).map_err(io)?;
    std::fs::set_permissions(script, perms).map_err(io)
}

/// 啟動檔裡 -Xmx 是幾 GB（M 換算成 GB 取到小數一位）；沒寫回 None。
pub fn read_ram_gb(script: &Path) -> Option<f64> {
    static RE: LazyLock<regex::bytes::Regex> = LazyLock::new(|| regex::bytes::Regex::new(r"-Xmx(\d+)([GgMm])\b").unwrap());
    let data = std::fs::read(script).ok()?;
    let m = RE.captures(&data)?;
    let n: f64 = std::str::from_utf8(&m[1]).ok()?.parse().ok()?;
    Some(if m[2].eq_ignore_ascii_case(b"g") { n } else { (n / 1024.0 * 10.0).round() / 10.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_splitlines() {
        assert_eq!(splitlines("a\r\nb\rc\n\nd"), ["a", "b", "c", "", "d"]);
        assert_eq!(splitlines("a\n"), ["a"]);
        assert!(splitlines("").is_empty());
    }

    #[test]
    fn sh_words() {
        assert_eq!(sh_split(r#""/opt/my jdk/java" -jar 's.jar'"#).unwrap(), ["/opt/my jdk/java", "-jar", "s.jar"]);
        assert_eq!(sh_split(r#"a "" b"#).unwrap(), ["a", "", "b"]);
        assert!(sh_split(r#"java "x"#).is_none());
    }

    #[test]
    fn crlf_shim_with_else_branch_is_not_removed() {
        // ) else ( 這種寫法不是 Forge 安裝器的格式，不能被當成檢查段落刪掉
        let t = "java -jar a-shim.jar --onlyCheckJava\r\nif %ERRORLEVEL% NEQ 0 (\r\n) else (\r\n)\r\njava -jar s.jar\r\n";
        assert_eq!(parse_bat(t, "run.bat", Path::new("C:\\s"), HashMap::new()).mode, Mode::Script);
    }
}
