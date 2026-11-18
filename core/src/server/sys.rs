//! 跟作業系統有關的程序操作：量記憶體（連子程序一起算）、強制結束整棵程序樹、電腦總記憶體。
use std::collections::HashMap;

/// 程序表 {pid: (父 pid, 記憶體 KB)}。
type Table = HashMap<u32, (u32, u64)>;

/// 連子程序一起算：cmd.exe／start.sh 和 Oracle 的 java.exe 都只是外殼，真正吃記憶體的 Java 在底下。
pub fn tree_mem_mb(pid: u32) -> Option<u64> {
    tree_kb(&table(), pid).map(|kb| (kb as f64 / 1024.0).round() as u64).filter(|&mb| mb > 0)
}

fn tree_kb(table: &Table, pid: u32) -> Option<u64> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&p, &(pp, _)) in table {
        children.entry(pp).or_default().push(p);
    }
    let (mut total, mut todo, mut seen) = (0, vec![pid], std::collections::HashSet::new());
    while let Some(p) = todo.pop() {
        if !seen.insert(p) {
            continue;
        }
        total += table.get(&p).map_or(0, |r| r.1);
        todo.extend(children.get(&p).into_iter().flatten().filter(|&&c| c != p));
    }
    (total > 0).then_some(total)
}

#[cfg(windows)]
fn table() -> Table {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    let mut out = Table::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut pe: PROCESSENTRY32W = std::mem::zeroed();
        pe.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut pe);
        while ok != 0 {
            out.insert(pe.th32ProcessID, (pe.th32ParentProcessID, 0));
            ok = Process32NextW(snap, &mut pe);
        }
        CloseHandle(snap);
        for (pid, row) in out.iter_mut() {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, *pid);
            if h.is_null() {
                continue;
            }
            let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            pmc.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if K32GetProcessMemoryInfo(h, &mut pmc, pmc.cb) != 0 {
                row.1 = pmc.WorkingSetSize as u64 / 1024;
            }
            CloseHandle(h);
        }
    }
    out
}

/// Mac：ps 失敗回空表。
#[cfg(not(windows))]
fn table() -> Table {
    let mut cmd = crate::platform::command("ps");
    cmd.args(["-A", "-o", "pid=,ppid=,rss="]);
    crate::platform::run_with_timeout(&mut cmd, std::time::Duration::from_secs(5)).map_or_else(Table::new, |o| parse_ps(&o.stdout))
}

#[cfg_attr(windows, allow(dead_code))]
fn parse_ps(out: &[u8]) -> Table {
    String::from_utf8_lossy(out)
        .lines()
        .filter_map(|l| {
            let p: Vec<u64> = l.split_whitespace().map(str::parse).collect::<Result<_, _>>().ok()?;
            (p.len() == 3).then(|| (p[0] as u32, (p[1] as u32, p[2])))
        })
        .collect()
}

/// 電腦實際裝了幾 GB；查不到當 8。
pub fn total_ram_gb() -> u64 {
    #[cfg(windows)]
    {
        let mut kb = 0u64;
        let ok = unsafe { windows_sys::Win32::System::SystemInformation::GetPhysicallyInstalledSystemMemory(&mut kb) };
        if ok != 0 { (kb as f64 / 1_048_576.0).round() as u64 } else { 8 }
    }
    #[cfg(not(windows))]
    {
        let mut cmd = crate::platform::command("sysctl");
        cmd.args(["-n", "hw.memsize"]);
        crate::platform::run_with_timeout(&mut cmd, std::time::Duration::from_secs(5))
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
            .map(|b| (b / 1_073_741_824.0).round() as u64)
            .filter(|&g| g > 0)
            .unwrap_or(8)
    }
}

/// 某個資料夾所在硬碟還剩多少空間（bytes）；查不到回 None。
pub fn disk_free(path: &std::path::Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut free = 0u64;
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut())
        };
        (ok != 0).then_some(free)
    }
    #[cfg(not(windows))]
    {
        let mut cmd = crate::platform::command("df");
        cmd.arg("-Pk").arg(path);
        crate::platform::run_with_timeout(&mut cmd, std::time::Duration::from_secs(5)).and_then(|o| parse_df(&o.stdout))
    }
}

/// `df -Pk` 第二行第四欄＝可用 KB。
#[cfg_attr(windows, allow(dead_code))]
fn parse_df(out: &[u8]) -> Option<u64> {
    String::from_utf8_lossy(out).lines().nth(1)?.split_whitespace().nth(3)?.parse::<u64>().ok().map(|kb| kb * 1024)
}

/// 強制結束整棵程序樹（最後手段）。Mac 的伺服器開機時已開在自己的程序群組裡，整組一起關。
pub fn kill_tree(pid: u32) {
    let mut cmd = if cfg!(windows) {
        let mut c = crate::platform::command("taskkill");
        c.args(["/PID", &pid.to_string(), "/T", "/F"]);
        c
    } else {
        let mut c = crate::platform::command("kill");
        c.args(["-KILL", "--", &format!("-{pid}")]);
        c
    };
    let killed = crate::platform::run_with_timeout(&mut cmd, std::time::Duration::from_secs(10)).is_some_and(|o| o.status.success());
    if !killed && cfg!(not(windows)) {
        let mut c = crate::platform::command("kill");
        c.args(["-KILL", &pid.to_string()]);
        let _ = crate::platform::run_with_timeout(&mut c, std::time::Duration::from_secs(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_sums_children_and_survives_loops() {
        let t: Table = [(10, (1, 1024)), (11, (10, 2048)), (12, (11, 1024)), (13, (99, 9999)), (14, (14, 5))].into();
        assert_eq!(tree_kb(&t, 10), Some(4096));
        assert_eq!(tree_kb(&t, 14), Some(5), "自己是自己的父程序也不能卡住");
        assert_eq!(tree_kb(&t, 77), None);
        assert_eq!(parse_ps(b"  1 0 100\n 22 1 2048\nbad line\n").len(), 2);
    }

    #[test]
    fn this_machine() {
        assert!(tree_mem_mb(std::process::id()).is_some_and(|mb| mb > 0));
        assert!(total_ram_gb() >= 1);
        assert!(disk_free(&std::env::temp_dir()).is_some_and(|b| b > 0));
        assert_eq!(parse_df(b"Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3 971350180 512 4096 80% /\n"), Some(4096 * 1024));
    }
}
