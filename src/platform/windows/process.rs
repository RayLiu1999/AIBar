//! 行程與連接埠探測：ToolHelp 列舉行程、`NtQueryInformationProcess` 讀命令列、
//! `GetExtendedTcpTable` 列出在聽的連接埠。

use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// `ProcessCommandLineInformation`（Windows 8.1 起）
const PROCESS_COMMAND_LINE_INFORMATION: PROCESSINFOCLASS = PROCESSINFOCLASS(60);

/// 執行檔名（小寫）符合條件的行程：pid 與命令列；讀不到命令列的略過
pub fn find(matches: impl Fn(&str) -> bool) -> Vec<(u32, String)> {
    let mut found = Vec::new();
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return found;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut ok = unsafe { Process32FirstW(snap, &mut entry) }.is_ok();
    while ok {
        let len = entry
            .szExeFile
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(entry.szExeFile.len());
        let exe = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
        if matches(&exe)
            && let Some(cmd) = command_line(entry.th32ProcessID)
        {
            found.push((entry.th32ProcessID, cmd));
        }
        ok = unsafe { Process32NextW(snap, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snap);
    }
    found
}

/// 讀取行程的命令列（`ProcessCommandLineInformation` 回傳 UNICODE_STRING 與其內容）
fn command_line(pid: u32) -> Option<String> {
    #[repr(C)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *const u16,
    }
    let handle: HANDLE = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut needed = 0u32;
    let _ = unsafe {
        NtQueryInformationProcess(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    let mut result = None;
    if needed as usize >= size_of::<UnicodeString>() {
        // 以 u64 配置以確保對齊
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        let status = unsafe {
            NtQueryInformationProcess(
                handle,
                PROCESS_COMMAND_LINE_INFORMATION,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                needed,
                &mut needed,
            )
        };
        if status.is_ok() {
            let us = unsafe { &*(buf.as_ptr() as *const UnicodeString) };
            if !us.buffer.is_null() {
                let chars = unsafe { std::slice::from_raw_parts(us.buffer, us.length as usize / 2) };
                result = Some(String::from_utf16_lossy(chars));
            }
        }
    }
    unsafe {
        let _ = CloseHandle(handle);
    }
    result
}

/// 行程在聽的 IPv4 TCP 連接埠
pub fn listening_ports(pid: u32) -> Vec<u16> {
    let mut size = 0u32;
    let _ = unsafe {
        GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if size == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u32; (size as usize).div_ceil(4)];
    let rc = unsafe {
        GetExtendedTcpTable(
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if rc != 0 {
        return Vec::new();
    }
    // MIB_TCPTABLE_OWNER_PID：dwNumEntries 之後緊接著各列
    let count = buf[0] as usize;
    let rows = unsafe { std::slice::from_raw_parts(buf.as_ptr().add(1) as *const MIB_TCPROW_OWNER_PID, count) };
    let mut ports: Vec<u16> = rows
        .iter()
        .filter(|r| r.dwOwningPid == pid)
        .map(|r| u16::from_be((r.dwLocalPort & 0xffff) as u16))
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}
