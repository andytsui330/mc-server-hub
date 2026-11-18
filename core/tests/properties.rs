use mc_hub_core::properties;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

fn fixture(bytes: &[u8]) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let root = std::env::var_os("MCHUB_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = root.join(format!(
        "properties-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("server.properties"), bytes).unwrap();
    dir
}

fn patch(dir: &std::path::Path, values: Value) -> Value {
    json!({"expected_revision":properties::read(dir).unwrap()["revision"], "values":values})
}

#[test]
fn metadata_exposes_fifteen_common_fields_without_guessing_missing_values() {
    let d = fixture(b"motd=Hi\nfuture-option=keep\n");
    let got = properties::read(&d).unwrap();
    let fields = got["fields"].as_array().unwrap();
    let expected = [
        "motd",
        "difficulty",
        "gamemode",
        "max-players",
        "pvp",
        "white-list",
        "online-mode",
        "server-port",
        "view-distance",
        "simulation-distance",
        "spawn-protection",
        "allow-flight",
        "enable-command-block",
        "force-gamemode",
        "level-seed",
    ];
    assert_eq!(
        fields
            .iter()
            .map(|f| f["key"].as_str().unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(
        fields
            .iter()
            .all(|f| !f["label"].as_str().unwrap().is_empty()
                && !f["description"].as_str().unwrap().is_empty())
    );
    assert_eq!(fields[0]["present"], true);
    assert_eq!(fields[1]["present"], false);
    assert_eq!(got["values"]["future-option"], "keep");
    assert!(got["values"].get("difficulty").is_none());
}

#[test]
fn edit_preserves_other_bytes_order_unknown_fields_and_exact_backup() {
    let original =
        b"# kept\r\n  motd : hello\r\nfuture = value\\:a\\=b\r\n! another comment\r\npvp=true\r\n";
    let d = fixture(original);
    let saved = properties::save(
        &d,
        &patch(
            &d,
            json!({"motd":"嗨 \\ # = :", "pvp":"false", "new-key":" added"}),
        ),
    )
    .unwrap();
    assert_eq!(saved["values"]["motd"], "嗨 \\ # = :");
    assert_eq!(saved["values"]["new-key"], " added");
    assert_eq!(fs::read(d.join("server.properties.bak")).unwrap(), original);
    let text = fs::read_to_string(d.join("server.properties")).unwrap();
    assert!(text.starts_with("# kept\r\n  motd : "));
    assert!(
        text.contains("\r\nfuture = value\\:a\\=b\r\n! another comment\r\npvp=false\r\nnew-key=")
    );
    let bytes = fs::read(d.join("server.properties")).unwrap();
    properties::save(&d, &patch(&d, json!({"motd":"嗨 \\ # = :"}))).unwrap();
    assert_eq!(
        fs::read(d.join("server.properties")).unwrap(),
        bytes,
        "same value must retain original representation"
    );
}

#[test]
fn java_escapes_continuations_separators_duplicates_and_unicode_are_decoded() {
    let d = fixture(b" # comment\\\n!also comment\nmotd=first\nmotd : \\u4F60\\u597D\\uD83D\\uDE00\\\n  continued\\\\tail\ncustom\\ key\\=x : value\\tend\njust-key\nempty=\n");
    let got = properties::read(&d).unwrap();
    assert_eq!(got["values"]["motd"], "你好😀continued\\tail");
    assert_eq!(got["values"]["custom key=x"], "value\tend");
    assert_eq!(got["values"]["just-key"], "");
    assert_eq!(got["values"]["empty"], "");
    let saved = properties::save(
        &d,
        &patch(&d, json!({"motd":"changed", "custom key=x":"safe:value=1"})),
    )
    .unwrap();
    assert_eq!(saved["values"]["motd"], "changed");
    assert_eq!(saved["values"]["custom key=x"], "safe:value=1");
    let text = fs::read_to_string(d.join("server.properties")).unwrap();
    assert!(text.contains("motd=first\nmotd : changed\n"));
}

#[test]
fn invalid_patches_do_not_touch_original_or_create_backup() {
    let d = fixture(b"motd=keep\npvp=true\n");
    for bad in [
        json!({"difficulty":"nightmare"}),
        json!({"gamemode":"5"}),
        json!({"pvp":"TRUE"}),
        json!({"server-port":"0"}),
        json!({"server-port":"65536"}),
        json!({"max-players":"-1"}),
        json!({"view-distance":"33"}),
        json!({"simulation-distance":"2"}),
        json!({"spawn-protection":"-1"}),
        json!({"motd":"hi\nonline-mode=false"}),
        json!({"future":"hi\r\ninjected=x"}),
        json!({"bad\nkey":"x"}),
        json!({"bad\u{0000}key":"x"}),
        json!({"":"x"}),
        json!({"pvp":true}),
    ] {
        assert!(
            properties::save(&d, &patch(&d, bad.clone())).is_err(),
            "accepted {bad}"
        );
        assert_eq!(
            fs::read(d.join("server.properties")).unwrap(),
            b"motd=keep\npvp=true\n"
        );
        assert!(!d.join("server.properties.bak").exists());
    }
}

#[test]
fn all_common_fields_save_and_advanced_values_escape_safely() {
    let d = fixture(b"# no defaults\n");
    let values = json!({"motd":"hello", "difficulty":"hard", "gamemode":"survival", "max-players":"20", "pvp":"true", "white-list":"true", "online-mode":"true", "server-port":"25565", "view-distance":"10", "simulation-distance":"10", "spawn-protection":"16", "allow-flight":"false", "enable-command-block":"false", "force-gamemode":"false", "level-seed":"-123 or a seed", "advanced=key:#":"#value\\nnot-line"});
    let saved = properties::save(&d, &patch(&d, values.clone())).unwrap();
    assert_eq!(saved["values"], values);
}

#[test]
fn stale_revision_is_rejected_without_overwriting_external_edit() {
    let d = fixture(b"motd=first\n");
    let old = patch(&d, json!({"motd":"ours"}));
    fs::write(d.join("server.properties"), b"motd=external\n").unwrap();
    let err = properties::save(&d, &old).unwrap_err();
    assert!(err.msg.contains("重新讀取"));
    assert_eq!(
        fs::read(d.join("server.properties")).unwrap(),
        b"motd=external\n"
    );
    assert!(!d.join("server.properties.bak").exists());
    assert!(properties::save(&d, &json!({"values":{"motd":"missing revision"}})).is_err());
}

#[test]
fn malformed_or_missing_files_are_not_replaced() {
    for data in [
        b"motd=\\uXXYY\n".as_slice(),
        b"motd=\\uD800\n",
        b"motd=a\\",
        b"motd=\0x",
        b"motd=\xff",
    ] {
        let d = fixture(data);
        assert!(properties::read(&d).is_err(), "accepted {data:?}");
        assert!(
            properties::save(&d, &json!({"expected_revision":"x","values":{"motd":"x"}})).is_err()
        );
        assert_eq!(fs::read(d.join("server.properties")).unwrap(), data);
    }
    let d = fixture(b"motd=x\n");
    let absent = d.join("absent");
    fs::create_dir(&absent).unwrap();
    assert!(
        properties::read(&absent)
            .unwrap_err()
            .msg
            .contains("第一次")
    );
    assert!(!absent.join("server.properties").exists());
}

#[test]
fn linked_directory_and_backup_are_rejected() {
    let d = fixture(b"motd=original\n");
    let target = fixture(b"motd=target\n");
    let link = d.join("linked");
    #[cfg(windows)]
    {
        assert!(
            std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&link)
                .arg(&target)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(properties::read(&link).is_err());
    assert!(properties::read(&link.join("..")).is_err());
    let backup = d.join("server.properties.bak");
    #[cfg(windows)]
    {
        assert!(
            std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&backup)
                .arg(&target)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &backup).unwrap();
    assert!(properties::save(&d, &patch(&d, json!({"motd":"changed"}))).is_err());
    assert_eq!(
        fs::read(d.join("server.properties")).unwrap(),
        b"motd=original\n"
    );
    assert_eq!(
        fs::read(target.join("server.properties")).unwrap(),
        b"motd=target\n"
    );
}

#[test]
fn continued_key_does_not_leave_a_backslash_in_the_replacement_prefix() {
    let d = fixture(b"motd\\\n =old\nnext-key\n");
    let saved =
        properties::save(&d, &patch(&d, json!({"motd":"new", "next-key":"added"}))).unwrap();
    assert_eq!(saved["values"]["motd"], "new");
    assert_eq!(saved["values"]["next-key"], "added");
    assert_eq!(
        properties::read(&d).unwrap()["values"],
        json!({"motd":"new","next-key":"added"})
    );
}
