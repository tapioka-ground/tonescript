//! 鳴らしながら EQ が効いているか、数で見る。
use std::sync::Arc;
use std::time::Duration;
use tonescript_engine::{Engine, Mixer};
use tonescript_render::arrange;

const SRC: &str = r#"
    let BPM = 120;
    let SECTIONS = [["A", 2, "p", "k", "m", 1.0]];
    let VOICES = #{ lead: #{ ch: 0, patch: "brightsaw" } };
    let MELODY = #{ "1": bar([[16, "A4"]]), "2": bar([[16, "A4"]]) };
    let ARRANGE = #{ "1": ["lead"], "2": ["lead"] };
    let GAINS = #{ lead: 1.0 };
"#;

fn pull(m: &mut Mixer, blocks: usize) -> Vec<f32> {
    let (mut l, mut r) = (vec![0.0; 1024], vec![0.0; 1024]);
    let mut out = Vec::new();
    for _ in 0..blocks {
        m.fill(&mut l, &mut r);
        out.extend_from_slice(&l);
        std::thread::sleep(Duration::from_millis(3));
    }
    out
}
fn rms(x: &[f32]) -> f32 { (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt() }
/// 6kHz から上だけ。**1段では緩すぎて低い所が漏れる**ので4段重ねる
fn highs(x: &[f32]) -> f32 {
    let mut b = x.to_vec();
    for _ in 0..4 {
        b = tonescript_dsp::filter::highpass(&b, 6000.0);
    }
    rms(&b)
}

fn main() {
    let s = Arc::new(tonescript_song::load_str(SRC).unwrap());
    let sc = Arc::new(arrange::build(&s).unwrap());
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    // **裏の音圧測定が終わるまで待つ。** 途中で効き始めると、
    // それだけで全部が大きくなって、EQ の差と区別がつかなくなる
    let t0 = std::time::Instant::now();
    while e.makeup() == 1.0 && t0.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("測った倍率 x{:.3}", e.makeup());
    // 天井に当てない。当たると潰れて、EQ の差が測れなくなる
    e.set_master_gain(0.05);
    // **先回り係が音を作り終えるのを待つ。** 待たずに測ると、
    // 鳴っていないぶんが混ざって比較にならない
    let measure = |e: &mut Engine, m: &mut Mixer| -> (f32, f32) {
        e.seek(0);
        std::thread::sleep(Duration::from_millis(250));
        e.play();
        let v = pull(m, 25);
        e.stop();
        (rms(&v), highs(&v))
    };
    let a = measure(&mut e, &mut m);
    println!("素のまま      実効 {:.5}  6k以上 {:.5}", a.0, a.1);

    for (name, lo, mid, hi) in [("素の再送", 0.0, 0.0, 0.0),
                                ("高域 -24", 0.0, 0.0, -24.0), ("低域 -24", -24.0, 0.0, 0.0),
                                ("中域 +12", 0.0, 12.0, 0.0)] {
        e.set_eq("lead", lo, mid, hi);
        std::thread::sleep(Duration::from_millis(150));
        let b = measure(&mut e, &mut m);
        println!(
            "{name}  実効 {:.5} ({:+.2} dB)   6k以上 {:.5} ({:+.2} dB)",
            b.0,
            20.0 * (b.0 / a.0).log10(),
            b.1,
            20.0 * (b.1 / a.1.max(1e-12)).log10()
        );
    }
}
