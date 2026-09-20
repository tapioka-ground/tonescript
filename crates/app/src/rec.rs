//! 録る。
//!
//! 受け口は音の出口と同じ約束で動く。**取り込む側のスレッドでは、待たない・
//! 確保しない・捨てない。** 自分の歌を録っている最中に固まると、その一発が
//! 台無しになる。
//!
//! ```text
//!   マイク ──→ 取り込み ──空箱を取って詰める──→ 書き溜める係
//!                  ↑                                │
//!                  └──────── 空箱を返す ────────────┘
//! ```
//!
//! 箱は最初に作っておいて、使い回す。
//!
//! 曲のどこから録ったか
//! --------------------
//! 書くのは**歌ったぶんだけ**で、頭に無音は足さない。どこから鳴らすかは
//! `AUDIO_TRACKS` の `at` が持つ。5小節目から歌えば `at` が5小節目を指す。
//!
//! 無音を足す手もあったが、そうすると「あとで置き場所を直す」ができない。
//! 位置は**データとして持つ**ほうが後から動かせる。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tonescript_dsp::osc::SR;
use tonescript_engine::ring::{ring, Rx, Tx};

/// 受け渡しに使う箱の大きさ（サンプル）。
const CHUNK: usize = 4096;
/// 箱の数。合計で 1 秒ぶんくらい。
const BOXES: usize = 16;

/// 取り込んだものを書き溜める係。
struct Sink {
    full: Rx<Vec<f32>>,
    empty: Tx<Vec<f32>>,
    /// 溜めたもの（左右そろえて交互）
    take: Vec<f32>,
    stop: Arc<AtomicBool>,
    len: Arc<AtomicUsize>,
}

impl Sink {
    fn run(mut self) -> Vec<f32> {
        loop {
            let mut got = false;
            while let Some(b) = self.full.pop() {
                self.take.extend_from_slice(&b);
                self.len.store(self.take.len(), Ordering::Relaxed);
                // 箱を返す。中身は消して長さだけ戻す
                let mut b = b;
                b.clear();
                let _ = self.empty.push(b);
                got = true;
            }
            if self.stop.load(Ordering::Relaxed) && !got && self.full.is_empty() {
                return self.take;
            }
            if !got {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }
}

/// 録る口。
pub struct Rec {
    stream: Option<cpal::Stream>,
    worker: Option<std::thread::JoinHandle<Vec<f32>>>,
    stop: Arc<AtomicBool>,
    /// 取り込んだ大きさ（画面の針に出す）
    level: Arc<AtomicU32>,
    /// 何サンプル溜まったか
    len: Arc<AtomicUsize>,
    pub name: Option<String>,
    pub error: Option<String>,
    pub channels: u16,
    pub sample_rate: u32,
}

impl Rec {
    /// ぶら下がっている受け口の名前。
    pub fn ports() -> Vec<String> {
        let Ok(host) = std::panic::catch_unwind(cpal::default_host) else { return Vec::new() };
        let Ok(list) = host.input_devices() else { return Vec::new() };
        list.filter_map(|d| d.name().ok()).collect()
    }

    /// 録り始める。
    ///
    /// どこから録ったかはここでは持たない。書くのは歌ったぶんだけで、
    /// 置き場所は `AUDIO_TRACKS` の `at` が持つ。
    pub fn start(which: Option<usize>) -> Rec {
        let stop = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0));
        let len = Arc::new(AtomicUsize::new(0));
        let mut me = Rec {
            stream: None,
            worker: None,
            stop: stop.clone(),
            level: level.clone(),
            len: len.clone(),
            name: None,
            error: None,
            channels: 1,
            sample_rate: SR as u32,
        };

        let host = cpal::default_host();
        let device = match which {
            Some(i) => host.input_devices().ok().and_then(|mut d| d.nth(i)),
            None => host.default_input_device(),
        };
        let Some(device) = device else {
            me.error = Some("録る口が見つかりません".into());
            return me;
        };
        me.name = device.name().ok();
        let cfg = match pick(&device) {
            Ok(c) => c,
            Err(e) => {
                me.error = Some(e);
                return me;
            }
        };
        me.channels = cfg.channels();
        me.sample_rate = cfg.sample_rate().0;

        // 箱を作って渡し合う
        let (full_tx, full_rx) = ring::<Vec<f32>>(BOXES * 2);
        let (empty_tx, empty_rx) = ring::<Vec<f32>>(BOXES * 2);
        for _ in 0..BOXES {
            let _ = empty_tx.push(Vec::with_capacity(CHUNK));
        }

        let lv = level.clone();
        let mut spare: Option<Vec<f32>> = None;
        let mut take = move |data: &[f32]| {
            let mut peak = 0.0f32;
            for v in data {
                peak = peak.max(v.abs());
            }
            // 針は大きいほうを残す。画面が読むまでのあいだの山が要る
            let was = f32::from_bits(lv.load(Ordering::Relaxed));
            if peak > was {
                lv.store(peak.to_bits(), Ordering::Relaxed);
            }
            let mut b = match spare.take().or_else(|| empty_rx.pop()) {
                Some(b) => b,
                // 箱が無い。ここで作ると詰まるので、この回は捨てる
                None => return,
            };
            b.extend_from_slice(data);
            if b.len() >= CHUNK {
                let _ = full_tx.push(b);
            } else {
                spare = Some(b);
            }
        };

        let err = |e| eprintln!("[録音] {e}");
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &cfg.into(),
                move |d: &[f32], _: &_| take(d),
                err,
                None,
            ),
            cpal::SampleFormat::I16 => {
                let mut buf = Vec::new();
                device.build_input_stream(
                    &cfg.into(),
                    move |d: &[i16], _: &_| {
                        buf.clear();
                        buf.extend(d.iter().map(|v| *v as f32 / 32768.0));
                        take(&buf);
                    },
                    err,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let mut buf = Vec::new();
                device.build_input_stream(
                    &cfg.into(),
                    move |d: &[u16], _: &_| {
                        buf.clear();
                        buf.extend(d.iter().map(|v| *v as f32 / 32768.0 - 1.0));
                        take(&buf);
                    },
                    err,
                    None,
                )
            }
            f => {
                me.error = Some(format!("扱えない形式です: {f:?}"));
                return me;
            }
        };
        match stream {
            Ok(s) => {
                if let Err(e) = s.play() {
                    me.error = Some(e.to_string());
                    return me;
                }
                me.stream = Some(s);
            }
            Err(e) => {
                me.error = Some(e.to_string());
                return me;
            }
        }
        let sink = Sink {
            full: full_rx,
            empty: empty_tx,
            take: Vec::with_capacity(SR as usize * 30),
            stop,
            len,
        };
        me.worker = Some(std::thread::spawn(move || sink.run()));
        me
    }

    pub fn is_open(&self) -> bool {
        self.stream.is_some()
    }

    /// 今の入力の大きさ。読むと 0 に戻る。
    pub fn take_level(&self) -> f32 {
        f32::from_bits(self.level.swap(0, Ordering::Relaxed))
    }

    /// 録れた長さ（秒）。
    pub fn seconds(&self) -> f32 {
        let n = self.len.load(Ordering::Relaxed) as f32 / self.channels.max(1) as f32;
        n / self.sample_rate.max(1) as f32
    }

    /// 録り終える。左右そろえて返す。歌ったぶんだけで、頭は足さない。
    pub fn finish(mut self) -> Result<(Vec<f32>, Vec<f32>), String> {
        drop(self.stream.take()); // 先に止める
        self.stop.store(true, Ordering::Relaxed);
        let raw = match self.worker.take() {
            Some(h) => h.join().map_err(|_| "録った音を受け取れませんでした".to_string())?,
            None => return Err("録れていません".into()),
        };
        if raw.is_empty() {
            return Err("何も入っていません（口は開いていますか）".into());
        }
        let (mut l, mut r) = split(&raw, self.channels);
        if self.sample_rate != SR as u32 {
            l = resample(&l, self.sample_rate, SR as u32);
            r = resample(&r, self.sample_rate, SR as u32);
        }
        Ok((l, r))
    }
}

/// 48kHz を頼む。通らなければ機械に任せる。
fn pick(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    let want = cpal::SampleRate(SR as u32);
    if let Ok(list) = device.supported_input_configs() {
        let mut best: Option<cpal::SupportedStreamConfigRange> = None;
        for c in list {
            if c.min_sample_rate() <= want && want <= c.max_sample_rate() {
                let better = match &best {
                    None => true,
                    // 少ないほうを採る。マイクは1本のことが多い
                    Some(b) => c.channels() < b.channels(),
                };
                if better {
                    best = Some(c);
                }
            }
        }
        if let Some(c) = best {
            return Ok(c.with_sample_rate(want));
        }
    }
    device.default_input_config().map_err(|e| e.to_string())
}

/// 交互に並んだものを左右へ分ける。1本しか無ければ両方に同じものを入れる。
pub fn split(raw: &[f32], channels: u16) -> (Vec<f32>, Vec<f32>) {
    match channels.max(1) {
        1 => (raw.to_vec(), raw.to_vec()),
        ch => {
            let ch = ch as usize;
            let n = raw.len() / ch;
            let mut l = Vec::with_capacity(n);
            let mut r = Vec::with_capacity(n);
            for i in 0..n {
                l.push(raw[i * ch]);
                r.push(raw[i * ch + 1]);
            }
            (l, r)
        }
    }
}

/// 周波数を合わせる。あいだは直線で埋める。
///
/// 凝った合わせ方はしていない。**まず合っていることのほうが大事**で、
/// 48kHz で録れる機械ならそもそもここを通らない。
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || from == 0 || x.is_empty() {
        return x.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let n = (x.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / ratio;
        let k = t as usize;
        let f = (t - k as f64) as f32;
        let a = x[k.min(x.len() - 1)];
        let b = x[(k + 1).min(x.len() - 1)];
        out.push(a + (b - a) * f);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_channel_goes_to_both_sides() {
        let (l, r) = split(&[1.0, 2.0, 3.0], 1);
        assert_eq!(l, vec![1.0, 2.0, 3.0]);
        assert_eq!(r, l, "1本なのに左右で違う");
    }

    #[test]
    fn two_channels_are_pulled_apart() {
        let (l, r) = split(&[1.0, -1.0, 2.0, -2.0], 2);
        assert_eq!(l, vec![1.0, 2.0]);
        assert_eq!(r, vec![-1.0, -2.0]);
    }

    #[test]
    fn extra_channels_keep_the_first_two() {
        let (l, r) = split(&[1.0, -1.0, 9.0, 2.0, -2.0, 9.0], 3);
        assert_eq!(l, vec![1.0, 2.0]);
        assert_eq!(r, vec![-1.0, -2.0]);
    }

    #[test]
    fn a_ragged_tail_is_dropped_not_guessed() {
        // 途中で切れた組は使わない
        let (l, r) = split(&[1.0, -1.0, 2.0], 2);
        assert_eq!(l.len(), 1);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn the_same_rate_is_left_alone() {
        let x = vec![1.0, 2.0, 3.0];
        assert_eq!(resample(&x, 48_000, 48_000), x);
        assert!(resample(&[], 44_100, 48_000).is_empty());
    }

    #[test]
    fn resampling_keeps_the_length_in_time() {
        // 44.1kHz で1秒ぶん入れたら、48kHz でも1秒ぶんになること
        let x = vec![0.0f32; 44_100];
        let y = resample(&x, 44_100, 48_000);
        assert!((y.len() as i32 - 48_000).abs() < 2, "{} サンプルになった", y.len());
    }

    #[test]
    fn resampling_keeps_the_pitch() {
        // 440Hz を入れたら 440Hz のまま。ゼロ交差の数で見る
        let from = 44_100u32;
        let hz = 440.0f32;
        let x: Vec<f32> =
            (0..from).map(|i| (i as f32 * std::f32::consts::TAU * hz / from as f32).sin()).collect();
        let y = resample(&x, from, 48_000);
        let cross = |s: &[f32]| s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        let (a, b) = (cross(&x), cross(&y));
        assert!((a as i32 - b as i32).abs() <= 1, "山の数が {a} から {b} に変わった");
    }
}
