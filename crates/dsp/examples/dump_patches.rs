use tonescript_dsp::patch::{self, Cfg};
use std::io::Write;
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let cfg = Cfg::default();
    let n = 12_000;
    for name in patch::NAMES {
        let y = patch::render(name, &cfg, 220.0, n, 0.9, 3).unwrap();
        let mut f = std::fs::File::create(format!("{dir}/p_{name}.f32")).unwrap();
        for x in &y { f.write_all(&x.to_le_bytes()).unwrap(); }
    }
}
