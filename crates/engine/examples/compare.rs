//! 一番単純な曲で、どこがずれるかを見る。
use std::sync::Arc;
use std::time::Duration;
use tonescript_dsp::osc::SR;
use tonescript_engine::{Engine, Mixer};
use tonescript_render::arrange;

const SRC: &str = r#"
    let BPM = 120;
    let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
    let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
    let MELODY = #{ "1": bar([[16, "A4"]]) };
    let ARRANGE = #{ "1": ["lead"] };
    let GAINS = #{ lead: 1.0 };
    let MASTER_LUFS = -9;
"#;

fn pull(m: &mut Mixer, blocks: usize, block: usize) -> Vec<f32> {
    let mut l = Vec::new();
    let (mut bl, mut br) = (vec![0.0; block], vec![0.0; block]);
    for _ in 0..blocks {
        m.fill(&mut bl, &mut br);
        l.extend_from_slice(&bl);
        std::thread::sleep(Duration::from_millis(3));
    }
    l
}

fn rms(x: &[f32]) -> f32 { (x.iter().map(|v| v*v).sum::<f32>() / x.len().max(1) as f32).sqrt() }
fn peak(x: &[f32]) -> f32 { x.iter().fold(0.0f32, |a,v| a.max(v.abs())) }

fn main() {
    let s = Arc::new(tonescript_song::load_str(SRC).unwrap());
    let sc = Arc::new(arrange::build(&s).unwrap());
    let quiet = |_: &str| {};
    println!("パート: {:?}", { let mut v: Vec<_> = sc.keys().collect(); v.sort(); v });
    for (k, v) in sc.iter() { println!("  {k}: {} ノート", v.len()); }

    let mut stems = tonescript_render::render_stems(&s, &sc, &quiet);
    for (k, v) in &stems { println!("ステム {k}: 実効 {:.5} ピーク {:.5}", rms(v), peak(v)); }
    let mut off = tonescript_render::mix_down(&s, &mut stems, &sc, &quiet);
    println!("ミックス後: 実効 {:.5} ピーク {:.5}", rms(&off.l), peak(&off.l));
    tonescript_render::master(&mut off, s.master_lufs, &quiet);
    println!("マスター後: 実効 {:.5} ピーク {:.5}", rms(&off.l), peak(&off.l));

    let (mut e, mut m) = Engine::new();
    e.set_song(s.clone(), sc.clone());
    std::thread::sleep(Duration::from_millis(800));
    e.play();
    println!("測った倍率 x{:.3}", e.makeup());
    let rl = pull(&mut m, (e.total() as usize / 1024) + 2, 1024);
    let n = off.l.len().min(rl.len());
    println!("再生: 実効 {:.5} ピーク {:.5}", rms(&rl[..n]), peak(&rl[..n]));

    let skip = (0.05 * SR) as usize;
    let (a, b) = (&off.l[skip..n], &rl[skip..n]);
    let ratio = rms(b) / rms(a).max(1e-9);
    println!("比 {:.3} 倍（{:+.2} dB）", ratio, 20.0 * ratio.log10());
    // 倍率を合わせたうえで、どれだけ形が違うか
    let err: f32 = a.iter().zip(b).map(|(x, y)| (x - y / ratio).abs()).sum::<f32>() / a.len() as f32;
    println!("倍率を揃えたあとの平均差 {:.6}（信号の実効 {:.5}）", err, rms(a));
    for i in [0usize, 4800, 24000, 48000] {
        if skip + i + 4 < n { println!("  {:>6}: 書き出し {:+.5} 再生 {:+.5}", i, a[i], b[i]); }
    }
}
