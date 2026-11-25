//! 執行命令列程式，並能終止整個行程樹（以 process group 實作）。

use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

/// 啟動前的設定（放進獨立的 process group）；啟動後以 `ProcessTree::attach` 納入
pub fn prepare(command: &mut Command) -> &mut Command {
    command.process_group(0)
}

/// 子行程與其後代；drop 時全部終止
pub struct ProcessTree(libc::pid_t);

impl ProcessTree {
    /// 把剛啟動的子行程納入（process group 的 id 就是子行程的 pid）
    pub fn attach(child: &Child) -> Option<ProcessTree> {
        // 0 與 1 對 killpg 有特殊意義，絕不可傳入
        libc::pid_t::try_from(child.id())
            .ok()
            .filter(|p| *p > 1)
            .map(ProcessTree)
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            libc::killpg(self.0, libc::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::process::Stdio;

    #[test]
    fn 終止留下的後代行程() {
        // 子行程立刻結束，留下持有 stdout 的後代；drop 後管線應立即關閉
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30 & echo ok"]).stdout(Stdio::piped());
        let mut child = prepare(&mut command).spawn().unwrap();
        let tree = ProcessTree::attach(&child).unwrap();
        child.wait().unwrap();
        let started = std::time::Instant::now();
        drop(tree);
        let mut out = String::new();
        child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
        assert_eq!(out, "ok\n");
        assert!(started.elapsed().as_secs() < 5);
    }
}
