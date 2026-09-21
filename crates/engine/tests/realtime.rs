//! 鳴らしながら計算する側が、**本当に動いているか**を確かめる。
//!
//! 音の出口は使わない。[`Mixer::fill`] を自分で呼んで、出てきた数字を見る。
//! 出口を開けない環境（CI や、音の無い機械）でも走る。
//!
//! 一番大事なのは最後の1つ。**聞こえる音と書き出す音が同じか。**

use std::sync::{Arc, Mutex, MutexGuard};
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

/// **この試験は1つずつ走らせる。**
///
/// どれも「間に合っているか」を時計で測っている。同時に何本も走らせると、
/// 測っているのは engine ではなく機械の混み具合になる。
/// 1本ずつ走る限り、これらは何度回しても同じ結果になる。
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn solo() -> MutexGuard<'static, ()> {
    // 前の試験が落ちて汚れていても、続きは走らせる
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

/// 設定を変えたあと、音側へ届くのを待つ。
///
/// 画面から触った設定は、係を通って音側へ渡る。届くまで数ミリ秒かかる
/// （鳴らしながら触れるようにした代償で、これは設計どおり）。
/// 試験で「変えた直後」を測るときは、届くのを待つ。
fn settle() {
    std::thread::sleep(Duration::from_millis(60));
}

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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
    let _one = solo();
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
fn recorded_audio_plays_and_stops_with_the_transport() {
    let _one = solo();
    use std::sync::Arc as A;
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));
    // 譜面のほうは黙らせる。外で録った音だけを見る
    for p in ["lead", "bass", "drums", "perc"] {
        e.set_audible(p, false);
    }
    e.set_master_gain(1.0);
    settle();

    let n = (2.0 * SR) as usize;
    let tone: Vec<f32> =
        (0..n).map(|i| 0.2 * (i as f32 * std::f32::consts::TAU * 440.0 / SR).sin()).collect();
    let track = |gain: f32, buf: &Vec<f32>| tonescript_engine::Track {
        label: "歌".into(),
        l: A::new(buf.clone()),
        r: A::new(buf.clone()),
        gain,
    };

    // まだ足していない。黙っているはず
    e.play();
    let (none, _) = pull(&mut m, 10, 1024, Duration::from_millis(2));
    assert!(peak(&none) < 1e-4, "譜面を黙らせたのに鳴っている: {}", peak(&none));

    // 足すと鳴る
    e.stop();
    e.seek(0);
    e.set_audio(vec![track(1.0, &tone)]);
    std::thread::sleep(Duration::from_millis(40));
    e.play();
    let (with, _) = pull(&mut m, 10, 1024, Duration::from_millis(2));
    assert!(peak(&with) > 0.05, "足した音が鳴っていない: {}", peak(&with));

    // 止めれば歌も止まる
    e.stop();
    let (quiet, _) = pull(&mut m, 4, 1024, Duration::from_millis(2));
    assert_eq!(peak(&quiet), 0.0, "止めたのに歌だけ鳴っている");

    // 音量 0 なら鳴らない
    e.seek(0);
    e.set_audio(vec![track(0.0, &tone)]);
    std::thread::sleep(Duration::from_millis(40));
    e.play();
    let (zero, _) = pull(&mut m, 10, 1024, Duration::from_millis(2));
    assert!(peak(&zero) < 1e-4, "音量 0 にしたのに鳴っている: {}", peak(&zero));
}

#[test]
fn recorded_audio_lines_up_with_the_song() {
    let _one = solo();
    use std::sync::Arc as A;
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));
    for p in ["lead", "bass", "drums", "perc"] {
        e.set_audible(p, false);
    }
    settle();
    // 頭の 0.5 秒は無音、そのあとだけ鳴る音を用意する
    let quiet = (0.5 * SR) as usize;
    let mut buf = vec![0.0f32; quiet];
    buf.extend((0..(1.0 * SR) as usize).map(|i| 0.3 * (i as f32 * 0.05).sin()));
    e.set_audio(vec![tonescript_engine::Track {
        label: "歌".into(),
        l: A::new(buf.clone()),
        r: A::new(buf),
        gain: 1.0,
    }]);
    std::thread::sleep(Duration::from_millis(40));

    // 0.7 秒目から鳴らす。頭出しした所の中身が出ること
    e.seek((0.7 * SR) as u64);
    e.play();
    let (l, _) = pull(&mut m, 8, 1024, Duration::from_millis(2));
    assert!(peak(&l) > 0.05, "頭出しした所の音が出ていない");

    // 0.1 秒目からなら、まだ無音の所
    e.stop();
    e.seek((0.1 * SR) as u64);
    e.play();
    let (early, _) = pull(&mut m, 4, 1024, Duration::from_millis(2));
    assert!(early.len() < quiet, "測る範囲が無音より長い");
    assert!(peak(&early) < 1e-4, "無音のはずの所で鳴った: {}", peak(&early));
}

#[test]
fn the_metronome_ticks_on_the_beat() {
    let _one = solo();
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));
    // 譜面は黙らせて、メトロノームだけを見る
    for p in ["lead", "bass", "drums", "perc"] {
        e.set_audible(p, false);
    }
    settle();
    // まず、切ってあれば鳴らない
    e.play();
    let (off, _) = pull(&mut m, 20, 1024, Duration::from_millis(3));
    assert!(peak(&off) < 1e-4, "切ってあるのに鳴った: {}", peak(&off));

    e.stop();
    e.seek(0);
    e.set_click(1.0);
    std::thread::sleep(Duration::from_millis(60));
    e.play();
    // 120BPM の4分音符 = 0.5秒ごと。2秒ぶんで4打
    let (on, _) = pull(&mut m, 100, 1024, Duration::from_millis(2));
    assert!(peak(&on) > 0.05, "メトロノームが鳴っていない");

    // 打点の数を数える。0.5秒ごとに山があること
    let win = (0.05 * SR) as usize;
    let mut hits = Vec::new();
    for beat in 0..4 {
        let at = (beat as f32 * 0.5 * SR) as usize;
        if at + win < on.len() {
            hits.push(peak(&on[at..at + win]));
        }
    }
    assert!(hits.len() >= 3, "測れるだけ鳴っていない");
    for (i, h) in hits.iter().enumerate() {
        assert!(*h > 0.05, "{i} 拍目に打点が無い（{h}）");
    }
    // 拍の間は静か
    let between = (0.25 * SR) as usize;
    assert!(
        peak(&on[between..between + win]) < hits[0] * 0.5,
        "拍と拍のあいだでも鳴りっぱなし"
    );
}

#[test]
fn counting_in_starts_the_song_by_itself() {
    let _one = solo();
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));
    e.set_click(1.0);

    // 4拍数えてから鳴り始める。120BPM なら 2 秒
    e.play_after_count(4);
    std::thread::sleep(Duration::from_millis(60));
    assert!(e.counting_in(), "数え始めていない");
    assert!(!e.is_playing(), "数える前に鳴り出した");

    // 1秒ぶん回しても、まだ曲は進んでいない
    pull(&mut m, 47, 1024, Duration::from_millis(1));
    assert_eq!(e.position(), 0, "数えている最中に曲が進んだ");

    // 残りを回すと、勝手に鳴り始める
    pull(&mut m, 60, 1024, Duration::from_millis(1));
    assert!(!e.counting_in(), "数え終わっていない");
    assert!(e.is_playing(), "数え終わったのに鳴り始めない");
    assert!(e.position() > 0, "位置が進んでいない");
}

#[test]
fn a_count_in_can_be_called_off() {
    let _one = solo();
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(60));
    e.play_after_count(8);
    std::thread::sleep(Duration::from_millis(40));
    assert!(e.counting_in());
    e.cancel_count();
    pull(&mut m, 20, 1024, Duration::from_millis(1));
    assert!(!e.counting_in());
    assert!(!e.is_playing(), "やめたのに鳴り出した");
    assert_eq!(e.position(), 0);
}

#[test]
fn the_metronome_never_reaches_the_file() {
    let _one = solo();
    // メトロノームは聞くためだけのもの。書き出しには入らない
    let (s, sc) = song();
    let quiet = |_: &str| {};
    let mut stems = tonescript_render::render_stems(&s, &sc, &quiet);
    let out = tonescript_render::mix_down(&s, &mut stems, &sc, &quiet);
    // 書き出し側にはメトロノームという言葉も無い（パートとして出てこない）
    assert!(!stems.contains_key("click"), "書き出しにメトロノームが入っている");
    assert!(out.len() > 0);
}

#[test]
fn seeking_does_not_stack_the_notes_on_top_of_each_other() {
    // **頭出ししても音量が変わらないこと。**
    //
    // 古い代の音を捨てる合図を誰も送っていなかったので、頭出しのたびに
    // 前の代がそのまま残り、同じ音符が二重に鳴って 6dB 大きくなっていた。
    // しかも頭出しを繰り返すほど増えた
    let _one = solo();
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    let t0 = Instant::now();
    while e.makeup() == 1.0 && t0.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(50));
    }
    settle();

    let mut take = |e: &mut Engine, m: &mut Mixer| -> f32 {
        e.seek(0);
        std::thread::sleep(Duration::from_millis(250));
        e.play();
        let (l, _) = pull(m, 25, 1024, Duration::from_millis(3));
        e.stop();
        rms(&l)
    };
    let first = take(&mut e, &mut m);
    assert!(first > 0.0, "そもそも鳴っていない");
    for round in 2..=4 {
        let again = take(&mut e, &mut m);
        let db = 20.0 * (again / first).log10();
        assert!(
            db.abs() < 1.0,
            "{round} 回目の頭出しで {db:+.2}dB 変わった（{first:.5} -> {again:.5}）"
        );
    }
}

#[test]
fn the_eq_changes_the_sound_while_it_plays() {
    let _one = solo();
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    // **裏の音圧測定が終わるまで待つ。** 途中で効き始めると、それだけで
    // 全部が大きくなって、EQ の差と区別がつかなくなる（実際それで
    // 「削ったのに大きくなった」という測定結果を出した）
    let t0 = Instant::now();
    while e.makeup() == 1.0 && t0.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(50));
    }
    settle();

    // 素のまま
    e.play();
    let (flat, _) = pull(&mut m, 30, 1024, Duration::from_millis(3));
    e.stop();
    e.seek(0);

    // 高い所を大きく削る。**鳴らしたまま効くこと**
    for p in ["lead", "bass", "drums", "perc"] {
        e.set_eq(p, -0.0, 0.0, -24.0);
    }
    settle();
    e.play();
    let (dull, _) = pull(&mut m, 30, 1024, Duration::from_millis(3));

    // **高い帯だけ取り出して測る。** 全体の実効値では、削った帯の
    // 割合が小さいと差が埋もれる
    //
    // ハイパスは**4段重ねる**。1段（6dB/oct）では緩すぎて低い所が漏れ、
    // 強い基音に埋もれて差が見えない（それで -0.8dB という読み違えをした）
    let highs = |x: &[f32]| -> f32 {
        let mut b = x.to_vec();
        for _ in 0..4 {
            b = tonescript_dsp::filter::highpass(&b, 6000.0);
        }
        rms(&b)
    };
    let (a, b) = (highs(&flat), highs(&dull));
    let db = 20.0 * (b / a.max(1e-12)).log10();
    assert!(db < -10.0, "6kHz から上が {db:+.1}dB しか減っていない（素 {a:.6} / 削り {b:.6}）");
    // 低い所は残っていること。全体が下がっただけでは EQ ではない
    let lows = |x: &[f32]| -> f32 {
        let hp = tonescript_dsp::filter::highpass(x, 300.0);
        let lo: Vec<f32> = x.iter().zip(&hp).map(|(v, h)| v - h).collect();
        (lo.iter().map(|v| v * v).sum::<f32>() / lo.len() as f32).sqrt()
    };
    let low_db = 20.0 * (lows(&dull) / lows(&flat).max(1e-12)).log10();
    assert!(low_db.abs() < 2.0, "低い所まで {low_db:+.1}dB 動いた");
}

#[test]
fn the_meters_say_what_actually_came_out() {
    let _one = solo();
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(80));

    // 止まっているあいだは振れない
    let _ = e.meters().take_master();
    pull(&mut m, 4, 1024, Duration::from_millis(2));
    assert_eq!(e.meters().take_master(), (0.0, 0.0), "止まっているのに針が振れた");

    e.play();
    let (l, r) = pull(&mut m, 30, 1024, Duration::from_millis(4));
    let (ml, mr) = e.meters().take_master();
    // 針は「読むまでの一番大きかった所」。実際に出た音と合うこと
    assert!((ml - peak(&l)).abs() < 1e-5, "左の針 {ml} / 実際 {}", peak(&l));
    assert!((mr - peak(&r)).abs() < 1e-5, "右の針 {mr} / 実際 {}", peak(&r));
    // 読んだら 0 に戻る
    assert_eq!(e.meters().take_master(), (0.0, 0.0));

    // パートごとの針も振れていること
    let lead = e.plan().part_of("lead").expect("lead が無い");
    pull(&mut m, 20, 1024, Duration::from_millis(4));
    assert!(e.meters().take_part(lead) > 0.0, "パートの針が振れない");
}

#[test]
fn the_loudness_meter_agrees_with_the_audio_it_measured() {
    let _one = solo();
    // 音圧計は 400ミリ秒の窓で今の音圧を出す。同じ音を書き出し側の物差しで
    // 測ったものと合うこと。
    //
    // 曲ぜんぶの音圧（`MASTER_LUFS`）とは別物。あちらは終わりの余白まで
    // 含めた平均なので、鳴っている最中の瞬時はそれより大きく出る
    let (s, sc) = song();
    let (mut e, mut m) = Engine::new();
    e.set_song(s, sc);
    std::thread::sleep(Duration::from_millis(400));
    e.play();
    let (l, r) = pull(&mut m, 60, 1024, Duration::from_millis(3));
    let now = e.meters().lufs();

    // 最後の 400ミリ秒を、書き出し側の lufs() で測る
    let win = (0.400 * SR) as usize;
    let from = l.len().saturating_sub(win);
    let want = tonescript_render::mix::lufs(&l[from..], &r[from..]);
    assert!((now - want).abs() < 1.0, "音圧計 {now:.2} / 同じ音を測ると {want:.2}");
    assert!(now > -40.0, "鳴っているのに {now:.1} LUFS");
}

#[test]
fn what_you_hear_is_what_gets_written() {
    let _one = solo();
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
