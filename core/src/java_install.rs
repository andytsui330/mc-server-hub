//! 只安裝至 Hub 私有 runtimes，永不修改系統 Java。
use crate::{Result, UserError, net, platform::Os};
use flate2::{
    Crc,
    read::{DeflateDecoder, GzDecoder},
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_ARCHIVE: u64 = 1 << 30;
const MAX_FILE: u64 = 512 << 20;
const MAX_TOTAL: u64 = 2 << 30;
const MAX_ENTRIES: usize = 50_000;
const MAX_DIRECTORY: u64 = 16 << 20;
static NEXT: AtomicU64 = AtomicU64::new(0);
fn bad(msg: &str) -> UserError {
    UserError::new(format!("Java 安裝已停止：{msg}"))
}
fn ioerr(e: io::Error) -> UserError {
    bad(&format!("無法讀寫安裝檔案：{e}"))
}

fn platform(os: Os, arch: &str) -> Result<(&'static str, &'static str)> {
    let os = match os {
        Os::Win => "windows",
        Os::Mac => "mac",
        _ => return Err(bad("只支援 Windows 與 Mac")),
    };
    let arch = match arch {
        "x64" => "x64",
        "aarch64" => "aarch64",
        _ => return Err(bad("不支援這個處理器架構")),
    };
    Ok((os, arch))
}

fn validate_release(r: &TemurinRelease) -> Result<()> {
    let (os, arch) = platform(r.os, &r.arch)?;
    if !(8..=999).contains(&r.major)
        || r.size == 0
        || r.size > MAX_ARCHIVE
        || r.sha256.len() != 64
        || !r.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(bad("官方版本、大小或 SHA-256 資料不完整"));
    }
    if r.version.is_empty()
        || r.version.len() > 120
        || !r
            .version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b))
        || r.version.to_ascii_lowercase().contains("ea")
        || version_major(&r.version) != Some(r.major)
    {
        return Err(bad("版本不是指定的正式版"));
    }
    let ext = if r.os == Os::Win { ".zip" } else { ".tar.gz" };
    let prefix = format!("OpenJDK{}U-jdk_{arch}_{os}_hotspot_", r.major);
    let (expected_tag, expected_version) = if r.major == 8 {
        let v = r
            .version
            .strip_prefix("1.8.0_")
            .ok_or_else(|| bad("Java 8 版本格式不正確"))?;
        let (update, build) = v
            .split_once("-b")
            .ok_or_else(|| bad("Java 8 版本格式不正確"))?;
        if update.is_empty()
            || build.is_empty()
            || !update
                .bytes()
                .chain(build.bytes())
                .all(|b| b.is_ascii_digit())
        {
            return Err(bad("Java 8 版本格式不正確"));
        }
        (
            format!("jdk8u{update}-b{build}"),
            format!("8u{update}b{build}"),
        )
    } else {
        let v = r.version.strip_suffix("-LTS").unwrap_or(&r.version);
        let (version, build) = v
            .split_once('+')
            .ok_or_else(|| bad("Java 版本缺少 build"))?;
        if version.is_empty()
            || build.is_empty()
            || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            || !build.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(bad("Java build 格式不正確"));
        }
        (format!("jdk-{v}"), format!("{version}_{build}"))
    };
    if r.archive_name != format!("{prefix}{expected_version}{ext}")
        || r.archive_name.len() > 200
        || !r
            .archive_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(bad("封裝檔名與指定的 JDK 平台不符"));
    }
    let prefix = format!(
        "https://github.com/adoptium/temurin{}-binaries/releases/download/",
        r.major
    );
    let rest = r
        .archive_url
        .strip_prefix(&prefix)
        .ok_or_else(|| bad("封裝來源不是指定的 Adoptium release repo"))?;
    let (tag, name) = rest
        .split_once('/')
        .ok_or_else(|| bad("官方下載路徑不完整"))?;
    let tag = tag.replace("%2B", "+").replace("%2b", "+");
    if name != r.archive_name || tag != expected_tag {
        return Err(bad("封裝下載路徑與指定版本不符"));
    }
    Ok(())
}

fn version_major(version: &str) -> Option<u32> {
    let v = version.strip_prefix("1.").unwrap_or(version);
    let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[derive(Clone, Debug, PartialEq)]
pub struct TemurinRelease {
    pub major: u32,
    pub version: String,
    pub os: Os,
    pub arch: String,
    pub archive_name: String,
    pub archive_url: String,
    pub sha256: String,
    pub size: u64,
}

pub fn resolve_temurin(major: u32, os: Os, arch: &str) -> Result<TemurinRelease> {
    resolve_temurin_with(major, os, arch, |url| {
        net::fetch_json_from(url, &net::Source::Temurin)
    })
}

pub fn resolve_temurin_with(
    major: u32,
    os: Os,
    arch: &str,
    mut fetch: impl FnMut(&str) -> Result<serde_json::Value>,
) -> Result<TemurinRelease> {
    let (os_name, arch) = platform(os, arch)?;
    if !(8..=999).contains(&major) {
        return Err(bad("Java 主版本不正確"));
    }
    let url = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture={arch}&image_type=jdk&os={os_name}&vendor=eclipse&release_type=ga&heap_size=normal"
    );
    let data = fetch(&url)?;
    for item in data
        .as_array()
        .ok_or_else(|| bad("官方版本清單格式不正確"))?
    {
        let b = &item["binary"];
        if item["vendor"] != "eclipse"
            || item.get("release_type").is_some_and(|x| x != "ga")
            || item["version"]["major"].as_u64() != Some(major as u64)
            || b["os"] != os_name
            || b["architecture"] != arch
            || b["image_type"] != "jdk"
            || b["jvm_impl"] != "hotspot"
            || b["heap_size"] != "normal"
        {
            continue;
        }
        let p = &b["package"];
        let r = TemurinRelease {
            major,
            os,
            arch: arch.into(),
            version: item["version"]["openjdk_version"]
                .as_str()
                .unwrap_or("")
                .into(),
            archive_name: p["name"].as_str().unwrap_or("").into(),
            archive_url: p["link"].as_str().unwrap_or("").into(),
            sha256: p["checksum"].as_str().unwrap_or("").to_ascii_lowercase(),
            size: p["size"].as_u64().unwrap_or(0),
        };
        validate_release(&r)?;
        return Ok(r);
    }
    Err(bad("找不到指定平台的正式 HotSpot JDK"))
}

pub fn install_temurin(
    data_dir: &Path,
    release: &TemurinRelease,
    confirmed: bool,
    progress: &mut dyn FnMut(f64),
) -> Result<PathBuf> {
    install_temurin_with(
        data_dir,
        release,
        confirmed,
        progress,
        |url, dest, checksum, size, progress| {
            net::download_from(
                url,
                dest,
                Some(checksum),
                Some(size),
                progress,
                &net::Source::Temurin,
            )
        },
    )
}

pub fn install_temurin_with(
    data_dir: &Path,
    release: &TemurinRelease,
    confirmed: bool,
    progress: &mut dyn FnMut(f64),
    mut download: impl FnMut(&str, &Path, &net::Checksum, u64, &mut dyn FnMut(f64)) -> Result<()>,
) -> Result<PathBuf> {
    if !confirmed {
        return Err(bad("尚未確認下載版本與來源"));
    }
    validate_release(release)?;
    fs::create_dir_all(data_dir).map_err(ioerr)?;
    plain_dir(data_dir)?;
    let runtimes = data_dir.join("runtimes");
    match fs::create_dir(&runtimes) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(ioerr(e)),
    }
    plain_dir(&runtimes)?;
    let (os, _) = platform(release.os, &release.arch)?;
    let name = format!(
        "temurin-{}-{os}-{}-{}",
        release.version,
        release.arch,
        &release.sha256[..16]
    );
    let target = runtimes.join(&name);
    let lock = runtimes.join(format!(".{name}.lock"));
    let _lock_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|_| bad("這個 runtime 正在安裝，請稍後再試"))?;
    let result = (|| {
        if fs::symlink_metadata(&target).is_ok() {
            return Err(bad("這個 runtime 已存在，保留原有檔案"));
        }
        let stage = loop {
            let p = runtimes.join(format!(
                ".stage-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&p) {
                Ok(()) => break p,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(ioerr(e)),
            }
        };
        let outcome = (|| {
            let archive = stage.join("download");
            download(
                &release.archive_url,
                &archive,
                &net::Checksum::Sha256(release.sha256.clone()),
                release.size,
                progress,
            )?;
            let meta = fs::symlink_metadata(&archive).map_err(ioerr)?;
            if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != release.size {
                return Err(bad("下載大小不符"));
            }
            let mut input = File::open(&archive).map_err(ioerr)?;
            let mut hash = Sha256::new();
            let mut buf = [0u8; 65536];
            loop {
                let n = input.read(&mut buf).map_err(ioerr)?;
                if n == 0 {
                    break;
                }
                hash.update(&buf[..n]);
            }
            if format!("{:x}", hash.finalize()) != release.sha256.to_ascii_lowercase() {
                return Err(bad("SHA-256 核對不符"));
            }
            let unpacked = stage.join("unpacked");
            fs::create_dir(&unpacked).map_err(ioerr)?;
            if release.os == Os::Win {
                extract_zip(&archive, &unpacked)?;
            } else {
                extract_tar(&archive, &unpacked)?;
            }
            let java = runtime_java(&unpacked, release.os, release.major)?;
            let relative = java
                .strip_prefix(&unpacked)
                .map_err(|_| bad("runtime 路徑不正確"))?
                .to_path_buf();
            if fs::symlink_metadata(&target).is_ok() {
                return Err(bad("目標 runtime 已存在"));
            }
            fs::rename(&unpacked, &target).map_err(ioerr)?;
            Ok(target.join(relative))
        })();
        // stage was freshly created by this operation and contains only validated regular files.
        let _ = fs::remove_dir_all(&stage);
        outcome
    })();
    drop(_lock_file);
    let _ = fs::remove_file(lock);
    result
}

fn plain_dir(p: &Path) -> Result<()> {
    let m = fs::symlink_metadata(p).map_err(ioerr)?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(bad("安裝目錄不能是連結"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err(bad("安裝目錄不能是重新解析點"));
        }
    }
    Ok(())
}

fn runtime_java(unpacked: &Path, os: Os, major: u32) -> Result<PathBuf> {
    let roots = fs::read_dir(unpacked)
        .map_err(ioerr)?
        .collect::<io::Result<Vec<_>>>()
        .map_err(ioerr)?;
    if roots.len() != 1 {
        return Err(bad("封裝必須包含單一 JDK 根目錄"));
    }
    let root = roots[0].path();
    plain_dir(&root)?;
    let home = if os == Os::Mac {
        root.join("Contents/Home")
    } else {
        root
    };
    if os == Os::Mac {
        plain_dir(home.parent().unwrap())?;
    }
    plain_dir(&home)?;
    plain_dir(&home.join("bin"))?;
    let release = home.join("release");
    let m = fs::symlink_metadata(&release).map_err(ioerr)?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() > 65536 {
        return Err(bad("JDK release 檔不正確"));
    }
    let text = fs::read_to_string(release).map_err(ioerr)?;
    let versions: Vec<_> = text
        .lines()
        .filter_map(|l| l.strip_prefix("JAVA_VERSION="))
        .collect();
    if versions.len() != 1 || version_major(versions[0].trim_matches('"')) != Some(major) {
        return Err(bad("JDK release 的 Java 主版本不符"));
    }
    let java = home.join(if os == Os::Win {
        "bin/java.exe"
    } else {
        "bin/java"
    });
    let m = fs::symlink_metadata(&java).map_err(ioerr)?;
    if !m.is_file() || m.file_type().is_symlink() || m.len() == 0 {
        return Err(bad("JDK 缺少 Java 執行檔"));
    }
    Ok(java)
}

/// 只掃描 Hub 自己的 runtimes；呼叫端可再用 java_major 核對實際執行版本。
pub fn installed_java(data_dir: &Path, need: u32) -> Vec<(u32, PathBuf)> {
    let dir = data_dir.join("runtimes");
    if plain_dir(&dir).is_err() {
        return vec![];
    }
    let mut found = vec![];
    for e in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(version) = name
            .strip_prefix("temurin-")
            .and_then(|s| s.split('-').next())
        else {
            continue;
        };
        let Some(major) = version_major(version) else {
            continue;
        };
        if major < need || plain_dir(&e.path()).is_err() {
            continue;
        }
        if let Ok(java) = runtime_java(&e.path(), crate::platform::OS, major) {
            found.push((major, java));
        }
    }
    found.sort_by_key(|e| e.0);
    found
}

fn safe_name(name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.len() > 1024 || name.contains('\\') || name.starts_with('/') {
        return Err(bad("封裝含不安全路徑"));
    }
    let name = name.strip_suffix('/').unwrap_or(name);
    let mut out = PathBuf::new();
    for part in name.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|c| c.is_control() || ":<>\"|?*".contains(c))
            || ["CON", "PRN", "AUX", "NUL", "CLOCK$"].contains(&stem.as_str())
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit())
        {
            return Err(bad("封裝含不安全路徑"));
        }
        out.push(part);
    }
    Ok(out)
}

struct Budget {
    seen: HashSet<String>,
    files: HashSet<String>,
    total: u64,
}
impl Budget {
    fn new() -> Self {
        Self {
            seen: HashSet::new(),
            files: HashSet::new(),
            total: 0,
        }
    }
    fn entry(&mut self, name: &str, size: u64, dir: bool) -> Result<PathBuf> {
        let path = safe_name(name)?;
        let key = path.to_string_lossy().replace('\\', "/").to_lowercase();
        if self.seen.len() >= MAX_ENTRIES
            || !self.seen.insert(key.clone())
            || size > MAX_FILE
            || self.total.checked_add(size).is_none_or(|s| s > MAX_TOTAL)
        {
            return Err(bad("封裝重複路徑或超過安全大小限制"));
        }
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            if self
                .files
                .contains(&parent.to_string_lossy().replace('\\', "/").to_lowercase())
            {
                return Err(bad("檔案與目錄路徑衝突"));
            }
        }
        if !dir {
            if self
                .seen
                .iter()
                .any(|p| p.starts_with(&(key.clone() + "/")))
            {
                return Err(bad("檔案與目錄路徑衝突"));
            }
            self.files.insert(key);
        }
        self.total += size;
        Ok(path)
    }
}

fn le16(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(b[p..p + 2].try_into().unwrap())
}
fn le32(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
struct ZipEntry {
    name: String,
    path: PathBuf,
    method: u16,
    crc: u32,
    packed: u64,
    size: u64,
    offset: u64,
    dir: bool,
}

fn extract_zip(archive: &Path, out: &Path) -> Result<()> {
    let mut f = File::open(archive).map_err(ioerr)?;
    let len = f.metadata().map_err(ioerr)?.len();
    let n = len.min(65557);
    f.seek(SeekFrom::Start(len - n)).map_err(ioerr)?;
    let mut tail = vec![0; n as usize];
    f.read_exact(&mut tail).map_err(ioerr)?;
    let at = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| {
            le32(&tail, i) == 0x06054b50 && i + 22 + le16(&tail, i + 20) as usize == tail.len()
        })
        .ok_or_else(|| bad("ZIP 目錄不完整"))?;
    let count = le16(&tail, at + 10) as usize;
    let dir_len = le32(&tail, at + 12) as u64;
    let dir_at = le32(&tail, at + 16) as u64;
    if count == 0
        || count >= 65535
        || count > MAX_ENTRIES
        || le16(&tail, at + 4) != 0
        || le16(&tail, at + 6) != 0
        || le16(&tail, at + 8) as usize != count
        || dir_len > MAX_DIRECTORY
        || dir_at + dir_len != len - n + at as u64
    {
        return Err(bad("ZIP64、分卷或過大目錄不支援"));
    }
    f.seek(SeekFrom::Start(dir_at)).map_err(ioerr)?;
    let mut dir = vec![0; dir_len as usize];
    f.read_exact(&mut dir).map_err(ioerr)?;
    let mut i = 0;
    let mut entries = vec![];
    let mut budget = Budget::new();
    let mut ranges = vec![];
    for _ in 0..count {
        if i + 46 > dir.len() || le32(&dir, i) != 0x02014b50 {
            return Err(bad("ZIP 目錄格式不正確"));
        }
        let n = le16(&dir, i + 28) as usize;
        let extra = le16(&dir, i + 30) as usize;
        let comment = le16(&dir, i + 32) as usize;
        let end = i + 46 + n + extra + comment;
        if end > dir.len() {
            return Err(bad("ZIP 目錄被截斷"));
        }
        let name = std::str::from_utf8(&dir[i + 46..i + 46 + n])
            .map_err(|_| bad("ZIP 檔名不是 UTF-8"))?
            .to_owned();
        let flags = le16(&dir, i + 8);
        let method = le16(&dir, i + 10);
        let mode = le32(&dir, i + 38) >> 16;
        let attrs = le32(&dir, i + 38);
        let is_dir = name.ends_with('/');
        let size = le32(&dir, i + 24) as u64;
        let packed = le32(&dir, i + 20) as u64;
        if flags & !0x0808 != 0
            || ![0, 8].contains(&method)
            || ![0, 0o040000, 0o100000].contains(&(mode & 0o170000))
            || (mode & 0o170000 == 0o040000 && !is_dir)
            || attrs & 0x400 != 0
            || (is_dir && size != 0)
            || (size > 0 && (packed == 0 || size > packed.saturating_mul(1000)))
            || le16(&dir, i + 34) != 0
        {
            return Err(bad("ZIP 含連結、特殊檔案、加密或解壓炸彈"));
        }
        let path = budget.entry(&name, size, is_dir)?;
        let e = ZipEntry {
            name,
            path,
            method,
            crc: le32(&dir, i + 16),
            packed,
            size,
            offset: le32(&dir, i + 42) as u64,
            dir: is_dir,
        };
        let mut head = [0; 30];
        f.seek(SeekFrom::Start(e.offset)).map_err(ioerr)?;
        f.read_exact(&mut head).map_err(ioerr)?;
        if le32(&head, 0) != 0x04034b50 || le16(&head, 6) != flags || le16(&head, 8) != method {
            return Err(bad("ZIP 本機檔頭不符"));
        }
        let name_len = le16(&head, 26) as usize;
        let mut local_name = vec![0; name_len];
        f.read_exact(&mut local_name).map_err(ioerr)?;
        if local_name != e.name.as_bytes() {
            return Err(bad("ZIP 檔名核對不符"));
        }
        let start = e.offset + 30 + name_len as u64 + le16(&head, 28) as u64;
        let end = start
            .checked_add(packed)
            .ok_or_else(|| bad("ZIP 大小溢位"))?;
        if end > dir_at {
            return Err(bad("ZIP 資料與目錄重疊"));
        }
        ranges.push((e.offset, end));
        entries.push((e, start));
        i = end_as_index(i, n, extra, comment);
    }
    if i != dir.len() {
        return Err(bad("ZIP 目錄有多餘資料"));
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|p| p[0].1 > p[1].0) {
        return Err(bad("ZIP 檔案資料重疊"));
    }
    for (e, start) in entries {
        let path = out.join(e.path);
        if e.dir {
            fs::create_dir_all(path).map_err(ioerr)?;
            continue;
        }
        fs::create_dir_all(path.parent().unwrap()).map_err(ioerr)?;
        let mut dest = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(ioerr)?;
        f.seek(SeekFrom::Start(start)).map_err(ioerr)?;
        let packed = (&mut f).take(e.packed);
        let reader: Box<dyn Read + '_> = if e.method == 0 {
            Box::new(packed)
        } else {
            Box::new(DeflateDecoder::new(packed))
        };
        copy_checked(reader, &mut dest, e.size, Some(e.crc))?;
    }
    Ok(())
}
fn end_as_index(i: usize, n: usize, extra: usize, comment: usize) -> usize {
    i + 46 + n + extra + comment
}

fn copy_checked(
    mut src: impl Read,
    dest: &mut File,
    size: u64,
    want_crc: Option<u32>,
) -> Result<()> {
    let mut left = size;
    let mut crc = Crc::new();
    let mut buf = [0; 65536];
    while left > 0 {
        let n = src
            .read(&mut buf[..left.min(65536) as usize])
            .map_err(ioerr)?;
        if n == 0 {
            return Err(bad("封裝檔案內容被截斷"));
        }
        dest.write_all(&buf[..n]).map_err(ioerr)?;
        crc.update(&buf[..n]);
        left -= n as u64;
    }
    let mut byte = [0];
    if src.read(&mut byte).map_err(ioerr)? != 0 || want_crc.is_some_and(|want| want != crc.sum()) {
        return Err(bad("解壓長度或 CRC 核對不符"));
    }
    dest.sync_all().map_err(ioerr)
}

fn tar_num(b: &[u8]) -> Result<u64> {
    let s = std::str::from_utf8(b)
        .map_err(|_| bad("tar 數字格式不正確"))?
        .trim_matches(['\0', ' ']);
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| bad("tar 數字格式不正確"))
}
fn tar_text(b: &[u8]) -> Result<String> {
    let end = b.iter().position(|&b| b == 0).unwrap_or(b.len());
    std::str::from_utf8(&b[..end])
        .map(str::to_owned)
        .map_err(|_| bad("tar 路徑不是 UTF-8"))
}

fn extract_tar(archive: &Path, out: &Path) -> Result<()> {
    let mut r = GzDecoder::new(File::open(archive).map_err(ioerr)?)
        .take(MAX_TOTAL + 64 * MAX_ENTRIES as u64 * 1024);
    let mut budget = Budget::new();
    let mut pending: Option<String> = None;
    let mut headers = 0usize;
    loop {
        let mut h = [0u8; 512];
        r.read_exact(&mut h).map_err(ioerr)?;
        headers += 1;
        if headers > MAX_ENTRIES * 2 {
            return Err(bad("tar 檔頭數量過多"));
        }
        if h == [0; 512] {
            let mut second = [0; 512];
            r.read_exact(&mut second).map_err(ioerr)?;
            if second != [0; 512] || pending.is_some() {
                return Err(bad("tar 結尾不正確"));
            }
            let mut rest = [0; 8192];
            let mut total = 0u64;
            loop {
                let n = r.read(&mut rest).map_err(ioerr)?;
                if n == 0 {
                    break;
                }
                total += n as u64;
                if total > 1 << 20 || rest[..n].iter().any(|&b| b != 0) {
                    return Err(bad("tar 結尾含多餘內容"));
                }
            }
            return Ok(());
        }
        let checksum = tar_num(&h[148..156])?;
        let actual: u64 = h
            .iter()
            .enumerate()
            .map(|(i, &b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    b as u64
                }
            })
            .sum();
        if checksum != actual {
            return Err(bad("tar 檔頭 checksum 不符"));
        }
        let size = tar_num(&h[124..136])?;
        let mode = tar_num(&h[100..108])?;
        let kind = h[156];
        let mut name = tar_text(&h[..100])?;
        let prefix = tar_text(&h[345..500])?;
        if !prefix.is_empty() {
            name = format!("{prefix}/{name}");
        }
        if matches!(kind, b'L' | b'x') {
            if pending.is_some() || size == 0 || size > 16384 {
                return Err(bad("tar 擴充路徑不正確"));
            }
            let mut data = vec![0; size as usize];
            r.read_exact(&mut data).map_err(ioerr)?;
            skip_padding(&mut r, size)?;
            let path = if kind == b'L' {
                tar_text(&data)?
            } else {
                pax_path(&data)?
            };
            safe_name(&path)?;
            pending = Some(path);
            continue;
        }
        if !matches!(kind, 0 | b'0' | b'5') || h[157..257].iter().any(|&b| b != 0) {
            return Err(bad("tar 含連結或特殊檔案"));
        }
        if let Some(path) = pending.take() {
            name = path;
        }
        let is_dir = kind == b'5';
        if is_dir && size != 0 {
            return Err(bad("tar 目錄含檔案資料"));
        }
        let path = out.join(budget.entry(&name, size, is_dir)?);
        if is_dir {
            fs::create_dir_all(path).map_err(ioerr)?;
        } else {
            fs::create_dir_all(path.parent().unwrap()).map_err(ioerr)?;
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(ioerr)?;
            copy_checked((&mut r).take(size), &mut f, size, None)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(
                    &path,
                    fs::Permissions::from_mode(if mode & 0o111 != 0 { 0o755 } else { 0o644 }),
                )
                .map_err(ioerr)?;
            }
            #[cfg(not(unix))]
            let _ = mode;
            skip_padding(&mut r, size)?;
        }
    }
}
fn skip_padding(r: &mut impl Read, size: u64) -> Result<()> {
    let mut b = vec![0; ((512 - size % 512) % 512) as usize];
    r.read_exact(&mut b).map_err(ioerr)
}
fn pax_path(data: &[u8]) -> Result<String> {
    let mut rest = data;
    let mut path = None;
    while !rest.is_empty() {
        let space = rest
            .iter()
            .position(|&b| b == b' ')
            .ok_or_else(|| bad("PAX 格式不正確"))?;
        let n = std::str::from_utf8(&rest[..space])
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .ok_or_else(|| bad("PAX 長度不正確"))?;
        if n <= space + 1 || n > rest.len() || rest[n - 1] != b'\n' {
            return Err(bad("PAX 長度不正確"));
        }
        let record =
            std::str::from_utf8(&rest[space + 1..n - 1]).map_err(|_| bad("PAX 不是 UTF-8"))?;
        let (key, value) = record
            .split_once('=')
            .ok_or_else(|| bad("PAX 欄位不正確"))?;
        match key {
            "path" if path.is_none() => path = Some(value.to_owned()),
            "mtime" | "atime" | "ctime" | "uid" | "gid" | "uname" | "gname" => {}
            _ => return Err(bad("PAX 含不支援的連結或特殊欄位")),
        }
        rest = &rest[n..];
    }
    path.ok_or_else(|| bad("PAX 缺少路徑"))
}
