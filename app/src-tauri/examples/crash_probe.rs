#[path = "../src/reports.rs"]
mod reports;
fn main() {
    let dir = std::path::PathBuf::from(std::env::var_os("MCHUB_DATA_DIR").expect("隔離資料夾"));
    assert!(dir.is_absolute() && dir.starts_with(std::env::temp_dir()));
    reports::session(&dir, false).unwrap();
    reports::resume(&dir).unwrap();
    reports::hook(dir);
    panic!("PRIVATE PANIC PAYLOAD must never appear in a report");
}
