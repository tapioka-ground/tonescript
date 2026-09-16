use tonescript_dsp::drum::{self, KickCfg};
use std::io::Write;

fn dump(dir: &str, name: &str, v: &[f32]) {
    let mut f = std::fs::File::create(format!("{dir}/{name}.f32")).unwrap();
    for x in v { f.write_all(&x.to_le_bytes()).unwrap(); }
}

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let n = 24_000;
    let cfg = KickCfg::default();
    dump(&dir, "d_kick", &drum::kick(n, 1.0, 0));
    dump(&dir, "d_hardkick", &drum::hardkick(n, 1.0, 0, &cfg, None));
    dump(&dir, "d_clap", &drum::clap(n, 1.0, 0));
    dump(&dir, "d_hat", &drum::hat(n, 1.0, 0.05, 0));
    dump(&dir, "d_crash", &drum::crash(n, 1.0, 0));
    dump(&dir, "d_snare", &drum::snare(n, 1.0, 0));
    dump(&dir, "d_tom", &drum::tom(n, 1.0, 180.0, 0));
    dump(&dir, "d_ride", &drum::ride(n, 1.0, 0));
    dump(&dir, "d_shaker", &drum::shaker(n, 1.0, 0));
    dump(&dir, "d_rim", &drum::rim(n, 1.0, 0));
    dump(&dir, "d_reverse", &drum::reverse(n, 1.0, 0));
}
