//! 讀 jar（zip）裡的小檔、核對整個 jar 完整（1.x 用 Python zipfile）。
//! 只自己讀目錄與解壓，不另加 zip 套件：要的只有「讀一個檔」和「逐檔核對 CRC」兩件事。
//! shortcut: 不支援 zip64（4 GB 以上或 6 萬 5 千個檔以上的 jar）與加密, 伺服器 jar 都遠小於此，遇到就當讀不到
use flate2::{read::DeflateDecoder, Crc};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// 單一檔案解壓後最多讀這麼大，防止壞掉或惡意的 jar 把記憶體吃光。
const MAX_ENTRY: u64 = 64 << 20;

struct Entry {
    name: String,
    method: u16,
    crc: u32,
    packed: u64,
    size: u64,
    offset: u64,
}

pub struct Jar {
    file: File,
    entries: Vec<Entry>,
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("不是正常的 jar：{what}"))
}

impl Jar {
    pub fn open(path: &Path) -> io::Result<Jar> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        // 結尾的「目錄位置」紀錄最多在檔尾 22＋65535 bytes 內
        let tail_len = len.min(22 + 65535);
        file.seek(SeekFrom::Start(len - tail_len))?;
        let mut tail = vec![0; tail_len as usize];
        file.read_exact(&mut tail)?;
        let end = (0..tail.len().saturating_sub(21)).rev().find(|&i| u32_at(&tail, i) == 0x0605_4b50).ok_or_else(|| bad("找不到目錄"))?;
        let (count, dir_len, dir_at) = (u16_at(&tail, end + 10) as usize, u32_at(&tail, end + 12) as u64, u32_at(&tail, end + 16) as u64);
        if dir_at == 0xFFFF_FFFF || dir_at + dir_len > len {
            return Err(bad("目錄位置不對"));
        }
        file.seek(SeekFrom::Start(dir_at))?;
        let mut dir = vec![0; dir_len as usize];
        file.read_exact(&mut dir)?;
        let (mut entries, mut i) = (Vec::with_capacity(count), 0);
        while entries.len() < count {
            if i + 46 > dir.len() || u32_at(&dir, i) != 0x0201_4b50 {
                return Err(bad("目錄內容不對"));
            }
            let (n, extra, comment) = (u16_at(&dir, i + 28) as usize, u16_at(&dir, i + 30) as usize, u16_at(&dir, i + 32) as usize);
            let name = dir.get(i + 46..i + 46 + n).ok_or_else(|| bad("檔名不完整"))?;
            entries.push(Entry {
                name: String::from_utf8_lossy(name).into_owned(),
                method: u16_at(&dir, i + 10),
                crc: u32_at(&dir, i + 16),
                packed: u32_at(&dir, i + 20) as u64,
                size: u32_at(&dir, i + 24) as u64,
                offset: u32_at(&dir, i + 42) as u64,
            });
            i += 46 + n + extra + comment;
        }
        Ok(Jar { file, entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    fn read_entry(&mut self, idx: usize) -> io::Result<Vec<u8>> {
        let e = &self.entries[idx];
        if e.size > MAX_ENTRY {
            return Err(bad("檔案太大"));
        }
        let mut head = [0u8; 30];
        self.file.seek(SeekFrom::Start(e.offset))?;
        self.file.read_exact(&mut head)?;
        if u32_at(&head, 0) != 0x0403_4b50 {
            return Err(bad("檔案位置不對"));
        }
        let skip = u16_at(&head, 26) as i64 + u16_at(&head, 28) as i64;
        self.file.seek(SeekFrom::Current(skip))?;
        let packed = (&mut self.file).take(e.packed);
        let mut out = Vec::with_capacity(e.size as usize);
        match e.method {
            0 => packed.take(e.size + 1).read_to_end(&mut out)?,
            8 => DeflateDecoder::new(packed).take(e.size + 1).read_to_end(&mut out)?,
            _ => return Err(bad("不認得的壓縮方式")),
        };
        let mut crc = Crc::new();
        crc.update(&out);
        if out.len() as u64 != e.size || crc.sum() != e.crc {
            return Err(bad(&format!("{} 核對不符", e.name)));
        }
        Ok(out)
    }

    /// 讀某個檔；jar 裡沒有這個檔回 Ok(None)。
    pub fn read(&mut self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match self.entries.iter().position(|e| e.name == name) {
            Some(i) => self.read_entry(i).map(Some),
            None => Ok(None),
        }
    }

    /// 每個檔都解壓並核對 CRC（1.x 的 testzip）；全部正確回 Ok(())。
    pub fn test(&mut self) -> io::Result<()> {
        for i in 0..self.entries.len() {
            if !self.entries[i].name.ends_with('/') {
                self.read_entry(i)?;
            }
        }
        Ok(())
    }
}

/// 測試用：寫一個 zip（`deflate` 為真就壓縮，否則原樣存）。
#[cfg(test)]
pub(crate) fn write_zip(path: &Path, files: &[(&str, &[u8], bool)]) {
    use flate2::{write::DeflateEncoder, Compression};
    use std::io::Write;
    let (mut out, mut dir) = (Vec::new(), Vec::new());
    for (name, data, deflate) in files {
        let mut crc = Crc::new();
        crc.update(data);
        let body = if *deflate {
            let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
            enc.write_all(data).unwrap();
            enc.finish().unwrap()
        } else {
            data.to_vec()
        };
        let common = |b: &mut Vec<u8>| {
            b.extend_from_slice(&20u16.to_le_bytes()); // 版本
            b.extend_from_slice(&0u16.to_le_bytes()); // 旗標
            b.extend_from_slice(&(if *deflate { 8u16 } else { 0 }).to_le_bytes());
            b.extend_from_slice(&[0; 4]); // 時間日期
            b.extend_from_slice(&crc.sum().to_le_bytes());
            b.extend_from_slice(&(body.len() as u32).to_le_bytes());
            b.extend_from_slice(&(data.len() as u32).to_le_bytes());
            b.extend_from_slice(&(name.len() as u16).to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes());
        };
        dir.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        dir.extend_from_slice(&20u16.to_le_bytes());
        common(&mut dir);
        dir.extend_from_slice(&[0; 10]); // 註解長度、磁碟、內部屬性、外部屬性
        dir.extend_from_slice(&(out.len() as u32).to_le_bytes());
        dir.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&body);
    }
    let at = out.len() as u32;
    out.extend_from_slice(&dir);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(dir.len() as u32).to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    std::fs::write(path, out).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_and_check() {
        let p = crate::fs_safe::tests::temp("jar").join("a.jar");
        let big = "game-version=1.21.1\n".repeat(200);
        write_zip(&p, &[("META-INF/", b"", false), ("version.json", br#"{"id":"1.21.1"}"#, false), ("install.properties", big.as_bytes(), true)]);
        let mut j = Jar::open(&p).unwrap();
        assert_eq!(j.names().collect::<Vec<_>>(), ["META-INF/", "version.json", "install.properties"]);
        assert_eq!(j.read("version.json").unwrap().unwrap(), br#"{"id":"1.21.1"}"#);
        assert_eq!(j.read("install.properties").unwrap().unwrap(), big.as_bytes());
        assert!(j.read("nope").unwrap().is_none());
        j.test().unwrap();
        // 壓縮內容被改一個 byte → 核對不符
        let mut data = std::fs::read(&p).unwrap();
        let at = data.windows(2).position(|w| w == b"1.").unwrap();
        data[at] = b'9';
        std::fs::write(&p, &data).unwrap();
        assert!(Jar::open(&p).unwrap().test().is_err());
        std::fs::write(&p, b"not a zip").unwrap();
        assert!(Jar::open(&p).is_err());
    }
}
