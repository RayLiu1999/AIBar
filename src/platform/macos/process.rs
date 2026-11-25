//! 行程與連接埠探測。尚未支援，見 `.scratch/macos-port` 工單 13。

pub fn find(_matches: impl Fn(&str) -> bool) -> Vec<(u32, String)> {
    Vec::new()
}

pub fn listening_ports(_pid: u32) -> Vec<u16> {
    Vec::new()
}
