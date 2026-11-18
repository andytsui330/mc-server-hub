use mc_hub_core::net::{Source, check_url_from};
#[test]
fn new_sources_are_separate_exact_https_policies() {
    for good in [
        "https://api.modrinth.com/v2/search?query=test",
        "https://cdn.modrinth.com/data/abc/versions/v/a.jar",
    ] {
        assert!(check_url_from(good, &Source::Modrinth).is_ok());
        assert!(check_url_from(good, &Source::Temurin).is_err());
    }
    for good in [
        "https://api.adoptium.net/v3/assets/latest/21/hotspot",
        "https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21/OpenJDK.zip",
        "https://release-assets.githubusercontent.com/a/file",
    ] {
        assert!(check_url_from(good, &Source::Temurin).is_ok());
        assert!(check_url_from(good, &Source::Modrinth).is_err());
    }
    for bad in [
        "http://api.modrinth.com/v2/search",
        "https://api.modrinth.com.evil/v2/search",
        "https://api.modrinth.com:443/v2/search",
        "https://github.com/evil/temurin21-binaries/releases/download/jdk/a.zip",
        "https://github.com/adoptium/temurin21-binaries/releases/download/../a.zip",
        "https://github.com/adoptium/temurin21-binaries/releases/download/%2e%2e/a.zip",
        "https://github.com/adoptium/temurin21-binaries/releases/download/jdk/%2fetc",
        "https://cdn.modrinth.com/else/a.jar",
    ] {
        assert!(check_url_from(bad, &Source::Modrinth).is_err(), "{bad}");
        assert!(check_url_from(bad, &Source::Temurin).is_err(), "{bad}");
    }
    assert!(check_url_from("https://api.adoptium.net/v3/x", &Source::Existing).is_err());
}

#[test]
fn oversized_stream_stops_before_writing_excess() {
    let root = std::env::temp_dir().join(format!(
        "mchub-p2-stream-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut progress = vec![];
    let result = mc_hub_core::net::save_stream(
        std::io::Cursor::new(vec![0u8; 100]),
        &root.join("file"),
        5,
        None,
        &mut |p| progress.push(p),
    );
    assert!(result.is_err());
    assert!(progress.iter().all(|p| *p <= 1.0));
    assert!(!root.join("file").exists());
}
