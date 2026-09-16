//! Python 版と数値を突き合わせるための書き出し。
//! 音を決めている所が移植でズレていないかを、耳ではなく数字で確かめる。
use tonescript_dsp::{env, filter, osc};
use std::io::Write;

fn dump(name: &str, v: &[f32]) {
    let dir = std::env::args().nth(1).expect("出力先を渡してください");
    let mut f = std::fs::File::create(format!("{dir}/{name}.f32")).unwrap();
    for x in v {
        f.write_all(&x.to_le_bytes()).unwrap();
    }
}

fn main() {
    let n = 4800;
    dump("saw440", &osc::saw(440.0, n, 0.0));
    dump("saw80", &osc::saw(80.0, n, 0.0));
    dump("square220", &osc::square(220.0, n, 0.0));
    dump("env_ad", &env::ad(n, 0.01, 0.2, 3.0));
    dump("env_lead", &env::lead(n, 0.004, 0.13, 0.52, 0.09));
    dump("env_adsr", &env::adsr(n, 0.01, 0.05, 0.4, 0.1));

    let x = osc::saw(220.0, n, 0.0);
    dump("lad_classic", &filter::ladder_classic(&x, &[1200.0], 0.3));
    dump("lad_zdf", &filter::ladder_zdf(&x, &[1200.0], 0.3, 0.0));
    dump("hp", &filter::highpass(&x, 400.0));
    dump("bp", &filter::bandpass(&x, 900.0, 4.0));

    // フィルタだけを比べるための入力。両方の言語で同じ値になるものを使う。
    // 波形テーブルを通すと入力自体が違ってしまい、フィルタの比較にならない。
    let sine: Vec<f32> = (0..n)
        .map(|i| (2.0 * std::f64::consts::PI * 220.0 * i as f64 / 48_000.0).sin() as f32)
        .collect();
    dump("ref_sine", &sine);
    dump("hp_sine", &filter::highpass(&sine, 400.0));
    dump("bp_sine", &filter::bandpass(&sine, 900.0, 4.0));
    dump("lad_classic_sine", &filter::ladder_classic(&sine, &[1200.0], 0.3));
    dump("lad_zdf_sine", &filter::ladder_zdf(&sine, &[1200.0], 0.3, 0.0));
}
