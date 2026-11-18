use mc_hub_core::{UserError, java_install::*, platform::Os};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
fn fixture() -> PathBuf {
    let base = PathBuf::from(
        std::env::var_os("MCHUB_DATA_DIR").expect("isolated MCHUB_DATA_DIR required"),
    );
    assert!(base.starts_with(std::env::temp_dir()));
    let d = base.join(format!(
        "java-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

fn metadata(os: &str, arch: &str, name: &str) -> serde_json::Value {
    json!([{"release_type":"ga", "vendor":"eclipse", "version":{"major":21,"openjdk_version":"21.0.8+9"}, "binary":{
        "os":os,"architecture":arch,"image_type":"jdk","jvm_impl":"hotspot","heap_size":"normal",
        "package":{"name":name,"link":format!("https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.8%2B9/{name}"), "checksum":"a".repeat(64),"size":1234}}}])
}

#[test]
fn resolves_only_requested_ga_hotspot_jdk_and_official_release_package() {
    let m = metadata(
        "windows",
        "x64",
        "OpenJDK21U-jdk_x64_windows_hotspot_21.0.8_9.zip",
    );
    let r = resolve_temurin_with(21, Os::Win, "x64", |url| {
        assert!(url.starts_with("https://api.adoptium.net/v3/assets/latest/21/hotspot?"));
        Ok(m.clone())
    })
    .unwrap();
    assert_eq!(r.version, "21.0.8+9");
    assert_eq!(r.size, 1234);
    let mut actual_shape = m.clone();
    actual_shape[0]
        .as_object_mut()
        .unwrap()
        .remove("release_type");
    assert!(resolve_temurin_with(21, Os::Win, "x64", |_| Ok(actual_shape.clone())).is_ok());
    for (pointer, bad) in [
        ("/0/release_type", json!("ea")),
        ("/0/vendor", json!("other")),
        ("/0/binary/os", json!("mac")),
        ("/0/binary/architecture", json!("aarch64")),
        ("/0/binary/image_type", json!("jre")),
        ("/0/binary/jvm_impl", json!("openj9")),
        ("/0/version/major", json!(17)),
        ("/0/binary/package/size", json!(0)),
        ("/0/version/openjdk_version", json!("21.0.9+9")),
        ("/0/binary/package/size", json!(u64::MAX)),
        ("/0/binary/package/checksum", json!("a")),
        (
            "/0/binary/package/link",
            json!("https://github.com/evil/temurin21-binaries/releases/download/jdk-21/a.zip"),
        ),
        (
            "/0/binary/package/link",
            json!("https://github.com/adoptium/temurin17-binaries/releases/download/jdk-21/a.zip"),
        ),
        ("/0/binary/package/name", json!("../java.zip")),
    ] {
        let mut changed = m.clone();
        *changed.pointer_mut(pointer).unwrap() = bad;
        assert!(
            resolve_temurin_with(21, Os::Win, "x64", |_| Ok(changed.clone())).is_err(),
            "{pointer}"
        );
    }
    assert!(
        resolve_temurin_with(21, Os::Other, "x64", |_| panic!(
            "unsupported OS must not fetch"
        ))
        .is_err()
    );
    let mut java8 = metadata(
        "windows",
        "x64",
        "OpenJDK8U-jdk_x64_windows_hotspot_8u512b09.zip",
    );
    java8[0]["version"] = json!({"major":8,"openjdk_version":"1.8.0_512-b09"});
    java8[0]["binary"]["package"]["link"] = json!(
        "https://github.com/adoptium/temurin8-binaries/releases/download/jdk8u512-b09/OpenJDK8U-jdk_x64_windows_hotspot_8u512b09.zip"
    );
    assert_eq!(
        resolve_temurin_with(8, Os::Win, "x64", |_| Ok(java8.clone()))
            .unwrap()
            .major,
        8
    );
}

// Hand-built stored ZIP, including Unix mode metadata, exercises the real reader.
fn zip(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
    let (mut out, mut dir) = (Vec::new(), Vec::new());
    for (name, data, mode) in files {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let mut common = Vec::new();
        common.extend_from_slice(&20u16.to_le_bytes());
        common.extend_from_slice(&[0; 8]);
        common.extend_from_slice(&crc.sum().to_le_bytes());
        common.extend_from_slice(&(data.len() as u32).to_le_bytes());
        common.extend_from_slice(&(data.len() as u32).to_le_bytes());
        common.extend_from_slice(&(name.len() as u16).to_le_bytes());
        common.extend_from_slice(&0u16.to_le_bytes());
        dir.extend_from_slice(&0x02014b50u32.to_le_bytes());
        dir.extend_from_slice(&0x0314u16.to_le_bytes());
        dir.extend_from_slice(&common);
        dir.extend_from_slice(&[0; 6]);
        dir.extend_from_slice(&mode.to_le_bytes());
        dir.extend_from_slice(&(out.len() as u32).to_le_bytes());
        dir.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        out.extend_from_slice(&common);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
    }
    let at = out.len() as u32;
    out.extend_from_slice(&dir);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(dir.len() as u32).to_le_bytes());
    out.extend_from_slice(&at.to_le_bytes());
    out.extend_from_slice(&[0; 2]);
    out
}

fn release(bytes: &[u8], os: Os) -> TemurinRelease {
    let name = if os == Os::Win {
        "OpenJDK21U-jdk_x64_windows_hotspot_21.0.8_9.zip"
    } else {
        "OpenJDK21U-jdk_x64_mac_hotspot_21.0.8_9.tar.gz"
    };
    let os_name = if os == Os::Win { "windows" } else { "mac" };
    let mut r =
        resolve_temurin_with(21, os, "x64", |_| Ok(metadata(os_name, "x64", name))).unwrap();
    r.size = bytes.len() as u64;
    r.sha256 = format!("{:x}", Sha256::digest(bytes));
    r
}

#[test]
fn requires_confirmation_then_installs_without_executing_fixture_or_overwriting() {
    let d = fixture();
    let bytes = zip(&[
        ("jdk/release", b"JAVA_VERSION=\"21.0.8\"\n", 0),
        ("jdk/bin/java.exe", b"FAKE: never execute", 0),
    ]);
    let r = release(&bytes, Os::Win);
    assert!(
        install_temurin_with(&d, &r, false, &mut |_| {}, |_, _, _, _, _| panic!(
            "unconfirmed download"
        ))
        .is_err()
    );
    assert!(!d.join("runtimes").exists());
    let p = install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
        fs::write(dest, &bytes).unwrap();
        Ok(())
    })
    .unwrap();
    assert!(p.starts_with(d.join("runtimes")));
    assert_eq!(fs::read(&p).unwrap(), b"FAKE: never execute");
    if mc_hub_core::platform::OS == Os::Win {
        assert_eq!(installed_java(&d, 21), vec![(21, p.clone())]);
        assert!(installed_java(&d, 22).is_empty());
    }
    assert!(
        install_temurin_with(&d, &r, true, &mut |_| {}, |_, _, _, _, _| panic!(
            "existing install should not download"
        ))
        .is_err()
    );
    assert_eq!(fs::read(&p).unwrap(), b"FAKE: never execute");
}

#[test]
fn rejects_bad_hash_crc_paths_links_duplicates_size_and_incomplete_runtime() {
    let d = fixture();
    fs::write(d.join("sentinel"), b"keep").unwrap();
    for files in [
        vec![("../escape", b"x".as_slice(), 0)],
        vec![("/absolute", b"x".as_slice(), 0)],
        vec![("C:/escape", b"x".as_slice(), 0)],
        vec![("jdk/link", b"target".as_slice(), 0o120777 << 16)],
        vec![("jdk/a", b"1".as_slice(), 0), ("jdk/A", b"2".as_slice(), 0)],
        vec![
            ("jdk/release", b"JAVA_VERSION=\"17\"".as_slice(), 0),
            ("jdk/bin/java.exe", b"fake".as_slice(), 0),
        ],
        vec![("jdk/release", b"JAVA_VERSION=\"21\"".as_slice(), 0)],
    ] {
        let bytes = zip(&files);
        let r = release(&bytes, Os::Win);
        assert!(
            install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
                fs::write(dest, &bytes).unwrap();
                Ok(())
            })
            .is_err()
        );
    }
    let bytes = zip(&[
        ("jdk/release", b"JAVA_VERSION=\"21\"", 0),
        ("jdk/bin/java.exe", b"fake", 0),
    ]);
    let mut r = release(&bytes, Os::Win);
    r.sha256 = "0".repeat(64);
    assert!(
        install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
            fs::write(dest, &bytes).unwrap();
            Ok(())
        })
        .is_err()
    );
    let mut crc_bad = bytes.clone();
    let i = crc_bad.windows(4).position(|b| b == b"fake").unwrap();
    crc_bad[i] = b'!';
    let r = release(&crc_bad, Os::Win);
    assert!(
        install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
            fs::write(dest, &crc_bad).unwrap();
            Ok(())
        })
        .is_err()
    );
    let mut bomb = bytes.clone();
    let i = bomb
        .windows(4)
        .position(|b| b == 0x02014b50u32.to_le_bytes())
        .unwrap();
    bomb[i + 24..i + 28].copy_from_slice(&u32::MAX.to_le_bytes());
    let r = release(&bomb, Os::Win);
    assert!(
        install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
            fs::write(dest, &bomb).unwrap();
            Ok(())
        })
        .is_err()
    );
    let r = release(&bytes, Os::Win);
    assert!(
        install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
            fs::write(dest, b"partial").unwrap();
            Err(UserError::new("offline"))
        })
        .is_err()
    );
    assert_eq!(fs::read(d.join("sentinel")).unwrap(), b"keep");
    assert!(!d.parent().unwrap().join("escape").exists());
    assert!(
        fs::read_dir(d.join("runtimes")).unwrap().next().is_none(),
        "failed staging must disappear"
    );
}

fn tar(files: &[(&str, &[u8], u8, &str)], declared_size: Option<u64>) -> Vec<u8> {
    use std::io::Write;
    let mut data = vec![];
    for (name, bytes, kind, link) in files {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..108].copy_from_slice(b"0000755\0");
        h[108..116].copy_from_slice(b"0000000\0");
        h[116..124].copy_from_slice(b"0000000\0");
        let size = declared_size.unwrap_or(bytes.len() as u64);
        h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
        h[136..148].copy_from_slice(b"00000000000\0");
        h[148..156].fill(b' ');
        h[156] = *kind;
        h[157..157 + link.len()].copy_from_slice(link.as_bytes());
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        data.extend_from_slice(&h);
        data.extend_from_slice(bytes);
        data.resize(data.len() + (512 - bytes.len() % 512) % 512, 0);
    }
    data.extend_from_slice(&[0; 1024]);
    let mut encoder = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
    encoder.write_all(&data).unwrap();
    encoder.finish().unwrap()
}

#[test]
fn mac_tar_install_is_checked_without_running_fixture_and_rejects_links_and_bombs() {
    let d = fixture();
    let bytes = tar(
        &[
            (
                "jdk-21.jdk/Contents/Home/release",
                b"JAVA_VERSION=\"21.0.8\"\n",
                b'0',
                "",
            ),
            (
                "jdk-21.jdk/Contents/Home/bin/java",
                b"not executable fixture",
                b'0',
                "",
            ),
        ],
        None,
    );
    let r = release(&bytes, Os::Mac);
    let p = install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
        fs::write(dest, &bytes).unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"not executable fixture");
    for (files, size) in [
        (vec![("../escape", b"x".as_slice(), b'0', "")], None),
        (vec![("/absolute", b"x".as_slice(), b'0', "")], None),
        (
            vec![("jdk/link", b"".as_slice(), b'2', "../../escape")],
            None,
        ),
        (
            vec![("jdk/link", b"".as_slice(), b'1', "jdk/release")],
            None,
        ),
        (vec![("jdk/fifo", b"".as_slice(), b'6', "")], None),
        (
            vec![
                ("jdk/a", b"1".as_slice(), b'0', ""),
                ("jdk/A", b"2".as_slice(), b'0', ""),
            ],
            None,
        ),
        (
            vec![
                ("jdk/a", b"1".as_slice(), b'0', ""),
                ("jdk/a/child", b"2".as_slice(), b'0', ""),
            ],
            None,
        ),
        (
            vec![("jdk/bomb", b"".as_slice(), b'0', "")],
            Some((512 << 20) + 1),
        ),
    ] {
        let bytes = tar(&files, size);
        let r = release(&bytes, Os::Mac);
        assert!(
            install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
                fs::write(dest, &bytes).unwrap();
                Ok(())
            })
            .is_err(),
            "{files:?}"
        );
    }
    assert_eq!(fs::read(&p).unwrap(), b"not executable fixture");
}

#[test]
fn rejects_archive_size_mismatch_and_does_not_touch_other_runtimes() {
    let d = fixture();
    let bytes = zip(&[
        ("jdk/release", b"JAVA_VERSION=\"21\"", 0),
        ("jdk/bin/java.exe", b"fake", 0),
    ]);
    fs::create_dir_all(d.join("runtimes/old")).unwrap();
    fs::write(d.join("runtimes/old/preserve"), b"old runtime").unwrap();
    let mut r = release(&bytes, Os::Win);
    r.size += 1;
    assert!(
        install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
            fs::write(dest, &bytes).unwrap();
            Ok(())
        })
        .is_err()
    );
    assert_eq!(
        fs::read(d.join("runtimes/old/preserve")).unwrap(),
        b"old runtime"
    );
    assert_eq!(fs::read_dir(d.join("runtimes")).unwrap().count(), 1);
}

#[test]
fn rejects_zip_directory_limits_and_inconsistent_local_header() {
    let d = fixture();
    let good = zip(&[
        ("jdk/release", b"JAVA_VERSION=\"21\"", 0),
        ("jdk/bin/java.exe", b"fake", 0),
    ]);
    for kind in 0..4 {
        let mut bytes = good.clone();
        let end = bytes.len() - 22;
        match kind {
            0 => {
                bytes[end + 8..end + 10].copy_from_slice(&50001u16.to_le_bytes());
                bytes[end + 10..end + 12].copy_from_slice(&50001u16.to_le_bytes());
            }
            1 => bytes[end + 12..end + 16].copy_from_slice(&(17u32 << 20).to_le_bytes()),
            2 => bytes[30] = b'X',
            _ => {
                let at = bytes
                    .windows(4)
                    .position(|b| b == 0x02014b50u32.to_le_bytes())
                    .unwrap();
                bytes[at + 8..at + 10].copy_from_slice(&1u16.to_le_bytes());
            }
        }
        let r = release(&bytes, Os::Win);
        assert!(
            install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
                fs::write(dest, &bytes).unwrap();
                Ok(())
            })
            .is_err()
        );
    }
}

#[test]
fn streams_deflated_jdk_member_above_jar_64_mib_limit() {
    use std::io::Write;
    let d = fixture();
    let mut bytes = zip(&[
        ("jdk/release", b"JAVA_VERSION=\"21\"", 0),
        ("jdk/bin/java.exe", b"fake", 0),
        ("jdk/lib/modules", b"", 0),
    ]);
    // A deterministic 65 MiB member with a moderate ratio, below the bomb threshold.
    let mut data = vec![0u8; 65 << 20];
    let mut seed = 12345u32;
    for b in &mut data {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *b = seed as u8;
    }
    let mut enc = flate2::write::DeflateEncoder::new(vec![], flate2::Compression::fast());
    enc.write_all(&data).unwrap();
    let packed = enc.finish().unwrap();
    let mut crc = flate2::Crc::new();
    crc.update(&data);
    let eocd = bytes.len() - 22;
    let central = u32::from_le_bytes(bytes[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
    let positions: Vec<_> = bytes
        .windows(4)
        .enumerate()
        .filter_map(|(i, b)| (b == 0x02014b50u32.to_le_bytes()).then_some(i))
        .collect();
    let entry = positions[2];
    let local = u32::from_le_bytes(bytes[entry + 42..entry + 46].try_into().unwrap()) as usize;
    let start = local + 30 + "jdk/lib/modules".len();
    bytes[local + 8..local + 10].copy_from_slice(&8u16.to_le_bytes());
    bytes[local + 14..local + 18].copy_from_slice(&crc.sum().to_le_bytes());
    bytes[local + 18..local + 22].copy_from_slice(&(packed.len() as u32).to_le_bytes());
    bytes[local + 22..local + 26].copy_from_slice(&(data.len() as u32).to_le_bytes());
    bytes[entry + 10..entry + 12].copy_from_slice(&8u16.to_le_bytes());
    bytes[entry + 16..entry + 20].copy_from_slice(&crc.sum().to_le_bytes());
    bytes[entry + 20..entry + 24].copy_from_slice(&(packed.len() as u32).to_le_bytes());
    bytes[entry + 24..entry + 28].copy_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.splice(start..start, packed.iter().copied());
    let eocd = bytes.len() - 22;
    bytes[eocd + 16..eocd + 20].copy_from_slice(&((central + packed.len()) as u32).to_le_bytes());
    let r = release(&bytes, Os::Win);
    let java = install_temurin_with(&d, &r, true, &mut |_| {}, |_, dest, _, _, _| {
        fs::write(dest, &bytes).unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(
        fs::metadata(java.parent().unwrap().parent().unwrap().join("lib/modules"))
            .unwrap()
            .len(),
        65 << 20
    );
}
