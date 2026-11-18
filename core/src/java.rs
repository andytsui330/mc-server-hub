//! Java：偵測已安裝的版本、依遊戲版本判斷需要幾版。
use crate::platform::{self, Os, OS};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// 舊版號（1.x）查表；新版號（26.1 起）不在表內，靠版本資料或開機錯誤翻譯。
pub fn java_for_mc(version: &str) -> Option<u32> {
    let rest = version.strip_prefix("1.")?;
    let mut parts = rest.split('.');
    let num = |s: Option<&str>| s.filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).and_then(|s| s.parse::<u64>().ok());
    let minor = num(parts.next())?;
    let patch = match parts.next() {
        None => 0,
        some => num(some)?,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(if (minor, patch) >= (20, 5) { 21 } else if minor >= 18 { 17 } else if minor >= 17 { 16 } else { 8 })
}

/// 從 `java -version` 的輸出抓主版本：1.8 → 8，17.0.2 → 17。
pub fn parse_java_major(out: &[u8]) -> Option<u32> {
    let key = b"version \"";
    for i in 0..out.len().saturating_sub(key.len()) {
        if &out[i..i + key.len()] != key {
            continue;
        }
        let rest = &out[i + key.len()..];
        let digits = |s: &[u8]| s.iter().take_while(|b| b.is_ascii_digit()).count();
        let n = digits(rest);
        if n == 0 {
            continue;
        }
        let major: u32 = std::str::from_utf8(&rest[..n]).ok()?.parse().ok()?;
        if major != 1 || rest[..n] != *b"1" {
            return Some(major);
        }
        let after = &rest[n..];
        let m = if after.first() == Some(&b'.') { digits(&after[1..]) } else { 0 };
        return Some(if m == 0 { 0 } else { std::str::from_utf8(&after[1..=m]).ok()?.parse().ok()? });
    }
    None
}

pub fn java_major(exe: &Path) -> Option<u32> {
    let out = platform::run_with_timeout(platform::command(exe).arg("-version"), Duration::from_secs(20))?;
    parse_java_major(&[out.stderr, out.stdout].concat())
}

/// 掃 PATH 和常見安裝位置：剛裝好的 Java，平台不用重開也找得到。回傳 (主版本, 路徑)。
pub fn installed_javas() -> Vec<(u32, PathBuf)> {
    let paths = if OS == Os::Mac { mac_java_paths(&home()) } else { path_and_vendor_javas() };
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter(|p| seen.insert(if OS == Os::Win { p.to_string_lossy().to_lowercase().replace('/', "\\") } else { p.to_string_lossy().into_owned() }))
        .filter_map(|p| java_major(&p).filter(|&m| m > 0).map(|m| (m, p)))
        .collect()
}

/// 夠用裡最接近需求的版本（太新的 Java 舊模組容易出問題）。
pub fn best_java(need: Option<u32>, found: &[(u32, PathBuf)]) -> Option<(u32, PathBuf)> {
    found.iter().filter(|j| j.0 >= need.unwrap_or(8)).min_by_key(|j| j.0).cloned()
}

fn home() -> PathBuf {
    std::env::var_os(if OS == Os::Win { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default()
}

fn path_and_vendor_javas() -> Vec<PathBuf> {
    let exe = if OS == Os::Win { "java.exe" } else { "java" };
    let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join(exe)).find(|p| p.is_file()).into_iter().collect())
        .unwrap_or_default();
    if OS == Os::Win {
        let mut roots: Vec<PathBuf> = ["ProgramW6432", "ProgramFiles"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
        roots.extend(std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs")));
        for root in roots {
            for vendor in ["Java", "Eclipse Adoptium", "Eclipse Foundation", "AdoptOpenJDK", "Microsoft", "Zulu", "BellSoft", "Amazon Corretto", "Semeru"] {
                paths.extend(subdirs(&root.join(vendor)).map(|d| d.join("bin").join(exe)).filter(|p| p.is_file()));
            }
        }
    }
    paths
}

fn subdirs(dir: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir())
}

/// Mac 的 /usr/bin/java 只是轉接頭（沒裝 Java 時會跳安裝提示），所以直接找各 JDK 的實際位置。
fn mac_java_paths(home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for jvm in [PathBuf::from("/Library/Java/JavaVirtualMachines"), home.join("Library/Java/JavaVirtualMachines")] {
        paths.extend(subdirs(&jvm).map(|d| d.join("Contents/Home/bin/java")));
    }
    for brew in ["/opt/homebrew/opt", "/usr/local/opt"] {
        paths.extend(subdirs(Path::new(brew))
            .filter(|d| d.file_name().is_some_and(|n| n.to_string_lossy().starts_with("openjdk")))
            .map(|d| d.join("bin/java")));
    }
    if let Some(out) = platform::run_with_timeout(platform::command("/usr/libexec/java_home").arg("-V"), Duration::from_secs(10)) {
        paths.extend(parse_java_home_list(&String::from_utf8_lossy(&out.stderr)));
    }
    paths.retain(|p| p.is_file());
    paths
}

/// `java_home -V` 把清單印在 stderr，每行最後是 JDK 的 Home 路徑。
pub fn parse_java_home_list(text: &str) -> Vec<PathBuf> {
    const SUFFIX: &str = "/Contents/Home";
    text.lines()
        .filter_map(|line| {
            let line = line.trim_end();
            let end = line.strip_suffix(SUFFIX)?.len();
            let b = line.as_bytes();
            let start = (0..end.saturating_sub(1)).find(|&i| b[i] == b'/' && !b[i + 1].is_ascii_whitespace())?;
            Some(PathBuf::from(format!("{}/bin/java", &line[start..])))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_version_output() {
        assert_eq!(parse_java_major(b"java version \"1.8.0_391\"\nJava(TM) SE"), Some(8));
        assert_eq!(parse_java_major(b"openjdk version \"21.0.5\" 2024-10-15 LTS"), Some(21));
        assert_eq!(parse_java_major(b"openjdk version \"17\" 2021-09-14"), Some(17));
        assert_eq!(parse_java_major(b"Picked up JAVA_TOOL_OPTIONS\nopenjdk version \"25-ea\""), Some(25));
        assert_eq!(parse_java_major(b"version \"x\" then version \"11.0.2\""), Some(11));
        assert_eq!(parse_java_major(b"not java"), None);
    }

    #[test]
    fn pick_closest_enough() {
        let found = vec![(21, PathBuf::from("a")), (8, PathBuf::from("b")), (17, PathBuf::from("c"))];
        assert_eq!(best_java(Some(16), &found).unwrap().0, 17);
        assert_eq!(best_java(None, &found).unwrap().0, 8);
        assert!(best_java(Some(25), &found).is_none());
    }

    #[test]
    fn java_home_list() {
        let text = "Matching Java Virtual Machines (2):\n    21.0.5 (arm64) \"Eclipse Adoptium\" - \"OpenJDK 21.0.5\" /Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home\n    17 (x86_64) \"A/B\" - \"x\" /Users/me/Library/Java/JavaVirtualMachines/jdk 17/Contents/Home  \n/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home\n";
        let got = parse_java_home_list(text);
        assert_eq!(got[0], PathBuf::from("/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home/bin/java"));
        assert_eq!(got[1], PathBuf::from("/B\" - \"x\" /Users/me/Library/Java/JavaVirtualMachines/jdk 17/Contents/Home/bin/java"), "照 1.x 的比對規則：取最左邊的 /");
        assert_eq!(got.len(), 3);
    }

    /// 這台電腦實際找得到的 Java（只印出來，不判對錯）：cargo test -- --ignored --nocapture
    #[test]
    #[ignore]
    fn list_this_machine() {
        println!("{:?}", installed_javas());
    }
}
