#![windows_subsystem = "windows"]
fn main(){
    let args:Vec<_>=std::env::args_os().collect();
    let Some(i)=args.iter().position(|s|s=="--marker") else{return};
    let p=std::path::PathBuf::from(&args[i+1]);
    // 不接受測試根層以外的位置，也不建立系統設定、捷徑或登錄值。
    let temp=std::path::PathBuf::from(std::env::var_os("MCHUB_DATA_DIR").unwrap());
    assert!(temp.is_absolute()&&p.starts_with(&temp));
    std::fs::write(p,"fake-update-installed").unwrap();
}
