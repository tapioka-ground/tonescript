//! 鳴らしながら計算する側が、**本当に動いているか**を確かめる。
//!
//! 音の出口は使わない。[`Mixer::fill`] を自分で呼んで、出てきた数字を見る。
//! 出口を開けない環境（CI や、音の無い機械）でも走る。
//!
//! 一番大事なのは最後の1つ。**聞こえる音と書き出す音が同じか。**

use std::sync::Arc;
use std::time::{Duration, Instant};

use tonescript_dsp::osc::SR;
use tonescript_engine::{Engine, Mixer};
use tonescript_render::arrange;
use tonescript_song::Song;

const SRC: &str = r#"
    let TITLE = "engine test";
    let BPM = 120;
    let SECTIONS = [["A", 2, "plain", "light", "main", 1.0]];
    let VOICES = #{
        lead:  #{ ch: 0, patch: "piano" },
        bass:  #{ ch: 1, patch: "sub" },
        drums: #{ ch: 9 },
    };
    let BASS_PATTERNS = #{ plain: [[0, 4, 0], [8, 4, 0]] };
    let ARP_PATTERNS = #{ main: [[0, 2, 0, 0]] };
    let DRUM_KITS = #{ light: #{ kick: [36, [[0, 110], [8, 110]]] } };
    let Am = ["Am", ["A3", "C4", "E4"], "A2"];
    let CHORDS = #{ "1": Am, "2": Am };
    let MELODY = #{
        "1": bar([[4, "A4"], [4, "C5"], [8, "E5"]]),
        "2": bar([[8, "C5"], [8, "A4"]]),
    };
    let ARRANGE = #{
        "1": ["lead", "bass", "kick"],
        "2": ["lead", "bass", "kick"],
    };
    let GAINS = #{ lead: 1.0, bass: 1.0, drums: 1.0 };
    let MIX = #{ lead: #{ width: 0.0, reverb: 0.0, duck: 0.0 } };
"#;

fn song() -> (Arc<Song>, Arc<arrange::Score>) {
    let s = tonescript_song::load_str(SRC).expect("曲が読めない");
    let score = arrange::build(&s).expect("譜面が組めない");
    (Arc::new(s), Arc::new(score))
}

/// 出口の代わり。`block` サンプルずつ、間を空けて引き取る。
///
/// 間を空けるのは、先回り係に作る時間を与えるため。実際の出口も
/// 「1ブロック鳴らすあいだ待つ」ので、同じことをしている。
fn pull(m: &mut Mixer, blocks: usize, block: usize, pace: Duration) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let (mut bl, mut br) = (vec![0.0; block], vec![0.0; block]);
    for _ in 0..blocks {
        m.fill(&mut bl, &mut br);
        l.extend_from_slice(&bl);
        r.extend_from_slice(&br);
        if !pace.is_zero() {
            std::thread::sleep(pace);
        }
    }
    (l, r)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |a, v| a.max(v.abs()))
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn nothing_comes_out_until_play_is_pressed() {
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(60));
    let (l, r) = pull(&mut m, 8, 512, Duration::from_millis(2));
    assert_eq!(peak(&l), 0.0, "止まっているのに鳴った");
    assert_eq!(peak(&r), 0.0);
    assert_eq!(e.position(), 0, "止まっているのに進んだ");
}

#[test]
fn pressing_play_makes_sound() {
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    // 先回り係が最初のぶんを作るのを待つ
    std::thread::sleep(Duration::from_millis(80));
    e.play();
    let (l, _r) = pull(&mut m, 40, 1024, Duration::from_millis(4));
    assert!(peak(&l) > 0.01, "鳴っていない（ピーク {}）", peak(&l));
    assert!(e.position() > 0, "位置が進んでいない");
}

#[test]
fn a_key_press_sounds_even_while_stopped() {
    // **これが直したかったこと。** 止まっていても、触った音は返る
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(60));

    // まず、何もしなければ無音
    let (quiet, _) = pull(&mut m, 4, 1024, Duration::from_millis(2));
    assert_eq!(peak(&quiet), 0.0, "何もしていないのに鳴った");

    e.note_on("lead", 72, 110, 0.3);
    std::thread::sleep(Duration::from_millis(60));
    let (l, r) = pull(&mut m, 20, 1024, Duration::from_millis(2));
    assert!(peak(&l) > 0.01, "押した音が返らない（ピーク {}）", peak(&l));
    assert!(peak(&r) > 0.01);
    // 止まったままであること。押しただけで走り出さない
    assert!(!e.is_playing(), "音を出しただけで再生が始まった");
    assert_eq!(e.position(), 0, "音を出しただけで位置が動いた");
}

#[test]
fn a_key_sounds_quickly_after_it_is_pressed() {
    // 押してから音が出るまで。鍵盤は、ここが遅いと弾けない
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));

    let t0 = Instant::now();
    e.key_down("lead", 67, 100);
    let (mut bl, mut br) = (vec![0.0; 256], vec![0.0; 256]);
    let mut waited = Duration::ZERO;
    loop {
        m.fill(&mut bl, &mut br);
        if peak(&bl) > 0.001 {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
        waited = t0.elapsed();
        assert!(waited < Duration::from_millis(300), "押しても音が出ない");
    }
    // 1ブロック（256サンプル = 5.3ms）ぶんの粗さはある。
    // 50ms 以内なら、弾いていて遅れは感じない
    assert!(waited < Duration::from_millis(50), "音が出るまで {waited:?} 掛かった");
}

#[test]
fn a_long_hold_is_seamless() {
    // 押しっぱなしにすると、裏で作り足して繋いでいく。
    // **繋ぎ目で切れたり、跳ねたりしないこと。**
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));
    e.key_down("lead", 60, 110);
    // 最初のぶんが届くのを待つ（時計は鳴らし始めるまで進まない）
    std::thread::sleep(Duration::from_millis(60));

    // 最初のぶん（0.3秒）をまたいで、1.2 秒ぶん鳴らす
    let (l, _r) = pull(&mut m, 60, 1024, Duration::from_millis(3));
    let n = l.len();
    assert!(peak(&l[..1024]) > 0.005, "鳴り出していない");
    // 継ぎ目があるあたり（0.19秒 = 0.3秒の62%）でも途切れていないこと
    let seam = (0.186 * SR) as usize;
    let win = 1024;
    assert!(
        rms(&l[seam..seam + win]) > 0.001,
        "継ぎ目で消えた（{}）",
        rms(&l[seam..seam + win])
    );
    // 隣り合う窓で急に倍以上／半分以下にならないこと
    let step = 512;
    let mut prev = rms(&l[..step]);
    for i in (step..n - step).step_by(step) {
        let now = rms(&l[i..i + step]);
        if prev > 0.01 && now > 0.01 {
            let jump = (now / prev).max(prev / now);
            assert!(jump < 3.0, "{:.3}秒で {jump:.1}倍 跳ねた", i as f32 / SR);
        }
        prev = now;
    }
    // 1.2 秒たっても鳴っていること（作り足しが効いている）
    let tail = (1.0 * SR) as usize;
    assert!(tail + win < n, "短すぎる");
    assert!(rms(&l[tail..tail + win]) > 0.0005, "作り足しが止まって消えた");
    e.key_up("lead", 60);
}

#[test]
fn a_held_key_keeps_sounding_until_it_is_released() {
    // 鍵盤を挿したときの道。押したら鳴り続け、離したら消える
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(60));

    e.key_down("lead", 67, 100);
    std::thread::sleep(Duration::from_millis(60));
    let (a, _) = pull(&mut m, 12, 1024, Duration::from_millis(2));
    assert!(peak(&a) > 0.01, "押しても鳴らない");

    // 押しっぱなし。まだ鳴っている
    let (b, _) = pull(&mut m, 12, 1024, Duration::from_millis(2));
    assert!(peak(&b) > 0.005, "押しっぱなしなのに消えた");

    e.key_up("lead", 67);
    std::thread::sleep(Duration::from_millis(40));
    // 消えるまで（下げるのに 0.025 秒）
    pull(&mut m, 8, 1024, Duration::from_millis(2));
    let (c, _) = pull(&mut m, 8, 1024, Duration::from_millis(2));
    assert!(peak(&c) < 1e-4, "離しても鳴り続けている（ピーク {}）", peak(&c));
}

#[test]
fn releasing_a_different_key_does_not_stop_this_one() {
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(60));
    e.key_down("lead", 67, 100);
    std::thread::sleep(Duration::from_millis(60));
    pull(&mut m, 4, 1024, Duration::from_millis(2));
    e.key_up("lead", 60); // 押していない音
    std::thread::sleep(Duration::from_millis(40));
    let (a, _) = pull(&mut m, 8, 1024, Duration::from_millis(2));
    assert!(peak(&a) > 0.005, "関係ない鍵で消えた");
}

#[test]
fn a_fader_moves_while_it_is_playing() {
    // 鳴らしたまま音量を変えて、本当に変わるか
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));
    e.play();
    let (loud, _) = pull(&mut m, 24, 1024, Duration::from_millis(4));

    e.set_master_gain(0.1);
    std::thread::sleep(Duration::from_millis(20));
    let (soft, _) = pull(&mut m, 24, 1024, Duration::from_millis(4));

    assert!(rms(&loud) > 0.0, "そもそも鳴っていない");
    assert!(
        rms(&soft) < rms(&loud) * 0.5,
        "下げたのに変わらない: {} -> {}",
        rms(&loud),
        rms(&soft)
    );
}

#[test]
fn muting_a_part_takes_effect_at_once() {
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));
    e.play();
    let (with, _) = pull(&mut m, 20, 1024, Duration::from_millis(4));

    for p in ["lead", "bass", "drums", "perc"] {
        e.set_audible(p, false);
    }
    std::thread::sleep(Duration::from_millis(20));
    let (without, _) = pull(&mut m, 20, 1024, Duration::from_millis(4));

    assert!(rms(&with) > 0.0, "そもそも鳴っていない");
    assert!(rms(&without) < rms(&with) * 0.2, "全部ミュートしたのに鳴っている");
}

#[test]
fn seeking_starts_from_there_and_drops_the_old_sound() {
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));
    e.play();
    pull(&mut m, 10, 1024, Duration::from_millis(4));

    let want = (0.9 * SR) as u64;
    e.seek(want);
    let (_l, _r) = pull(&mut m, 2, 256, Duration::from_millis(2));
    let now = e.position();
    assert!(now >= want && now < want + 4096, "頭出しした所から鳴っていない: {now}");
}

#[test]
fn the_end_of_the_song_stops_it() {
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(60));
    // 終わりの少し手前から
    e.seek(e.total().saturating_sub(2048));
    e.play();
    pull(&mut m, 8, 1024, Duration::from_millis(1));
    assert!(!e.is_playing(), "終わっても止まらない");
    assert!(e.took_end(), "終わったことが伝わっていない");
    assert!(!e.took_end(), "一度見たら下りるはず");
}

#[test]
fn a_loop_keeps_coming_back() {
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(60));
    let (from, to) = ((0.2 * SR) as u64, (0.6 * SR) as u64);
    e.set_loop(from, to);
    assert_eq!(e.loop_range(), Some((from, to)));
    e.seek(from);
    e.play();
    // 輪の長さ（0.4秒）の何倍も回す
    pull(&mut m, 60, 1024, Duration::from_millis(2));
    let pos = e.position();
    assert!(e.is_playing(), "繰り返しているのに止まった");
    assert!(pos >= from && pos <= to + 1024, "輪の外へ出た: {pos}");
}

#[test]
fn what_you_hear_is_what_gets_written() {
    // **この作りで一番大事な確かめ。**
    //
    // 鳴らしながら作った音と、書き出した音が同じ形になること。作る所を
    // 1つ（`render_note`）に保っているので、同じにならないほうがおかしい。
    //
    // 完全一致にはならない。書き出し側にはトラックごとの尖り止めが
    // 掛かっていて、あれは曲まるごとを見てからでないと決まらない。
    // なので「形が同じか」を相関で見る。
    let (s, sc) = song();

    // 書き出し側
    let quiet = |_: &str| {};
    let mut stems = tonescript_render::render_stems(&s, &sc, &quiet);
    let mut off = tonescript_render::mix_down(&s, &mut stems, &sc, &quiet);
    tonescript_render::master(&mut off, s.master_lufs, &quiet);

    // 鳴らす側
    let (mut e, mut m) = Engine::new();
    e.set_song(s.clone(), sc.clone());
    // 音圧の測り直しが終わるのを待つ（裏で曲を1回作っている）
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(50));
        if e.plan().total > 0 {
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(400));
    e.play();
    let blocks = (e.total() as usize / 1024) + 2;
    let (rl, rr) = pull(&mut m, blocks, 1024, Duration::from_millis(3));

    let n = off.l.len().min(rl.len());
    assert!(n > SR as usize, "短すぎる: {n}");

    // 頭の 0.1 秒は、係が追いつくまでの助走なので外す
    let skip = (0.1 * SR) as usize;
    let a = &off.l[skip..n];
    let b = &rl[skip..n];
    let corr = correlation(a, b);
    assert!(corr > 0.97, "書き出しと形が違う（相関 {corr:.4}）");

    // 右も同じ
    let corr_r = correlation(&off.r[skip..n], &rr[skip..n]);
    assert!(corr_r > 0.97, "右の形が違う（相関 {corr_r:.4}）");

    // 音圧も近いこと（裏で測った倍率が効いている）
    let (la, lb) = (rms(a), rms(b));
    let db = 20.0 * (lb.max(1e-9) / la.max(1e-9)).log10();
    assert!(db.abs() < 1.0, "音圧が {db:+.1}dB 違う（書き出し {la:.4} / 再生 {lb:.4}）");
}

fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let ma = a.iter().sum::<f32>() / n as f32;
    let mb = b.iter().sum::<f32>() / n as f32;
    let mut num = 0.0f64;
    let (mut da, mut db) = (0.0f64, 0.0f64);
    for i in 0..n {
        let (x, y) = ((a[i] - ma) as f64, (b[i] - mb) as f64);
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 0.0 || db <= 0.0 {
        return 0.0;
    }
    (num / (da.sqrt() * db.sqrt())) as f32
}
