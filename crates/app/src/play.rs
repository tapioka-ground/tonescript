//! 音の出口。
//!
//! ここは**受け口を開けるだけ**の薄い層で、音そのものは
//! [`tonescript_engine`] が作る。cpal に触るのはこのファイルだけに閉じる。
//!
//! 周波数を合わせる
//! ----------------
//! 音を作る側は 48kHz で動く。出口が 44.1kHz だと、そのまま流したときに
//! 曲が 8.8% 速くなり、音程も上がる。なので**出口側へ 48kHz を頼む**。
//! 断られた機械では既定のまま開けて、どれだけずれるかを言う（黙って
//! 半音ずれているより、言われたほうがいい）。
//!
//! 遅れのこと
//! ----------
//! 鍵盤を押してから音が出るまでの遅れは、ほとんどが**出口の塊の大きさ**で
//! 決まる。1回に 1024 サンプル渡す口なら、それだけで 21ms 出る。小さく
//! するほど速くなるが、間に合わなかったときにプツプツ鳴る。
//!
//! 頼んだ大きさがそのまま通るとは限らない。Windows の既定の口
//! （WASAPI の共有）は機械側の都合で決まる所があって、頼んでも
//! 動かないことがある。なので**実際に来た大きさを数えて出す**。
//! 頼んだ値を出すと、効いていないのに効いているように見える。
//!
//! ASIO
//! ----
//! Windows で本当に詰めたいなら ASIO の口が要る。ただし ASIO SDK が
//! 無いと**ビルドすら通らない**ので、既定では入れていない。入れるには
//! `--features asio`（詳しくは README）。入っていれば下の一覧に出る。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tonescript_dsp::osc::SR;
use tonescript_engine::Mixer;

/// どの口をどう開けるか。空文字と 0 は「おまかせ」。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Prefs {
    /// cpal の host 名（`WASAPI` / `ASIO` など）
    pub host: String,
    /// 機械の名前
    pub device: String,
    /// 1回に渡すサンプル数。0 でおまかせ
    pub buffer: u32,
}

impl Prefs {
    /// 選べる塊の大きさ。**2の冪だけ**にする。
    ///
    /// 半端な数を許すと、機械が勝手に近い値へ丸めたときに、頼んだ値と
    /// 出てくる値が食い違って見える
    pub const SIZES: [u32; 6] = [64, 128, 256, 512, 1024, 2048];
}

/// 開いている出口。落とすと止まる。
pub struct Out {
    #[allow(dead_code)]
    stream: Option<cpal::Stream>,
    /// 開けなかった理由。編集と書き出しは続けられる
    pub error: Option<String>,
    /// 実際に開いた周波数
    pub sample_rate: u32,
    /// 48kHz が通らなかったときの注意書き
    pub note: Option<String>,
    /// 実際に開いた口の名前
    pub host: String,
    pub device: String,
    /// 頼んだ塊の大きさ（0 はおまかせ）
    pub asked: u32,
    /// **実際に来た塊のうち一番大きかったもの。** 音側が数える
    frames: Arc<AtomicU32>,
}

impl Out {
    /// 何も頼まずに開ける。**試験用。**
    ///
    /// 道具のほうは必ず設定を添えて開ける（[`Out::open_with`]）ので、
    /// ここを通るのは試験だけ
    #[cfg(test)]
    pub fn start(mixer: Mixer) -> Out {
        Out::open_with(mixer, &Prefs::default())
    }

    /// 頼みを添えて開ける。
    pub fn open_with(mixer: Mixer, prefs: &Prefs) -> Out {
        let frames = Arc::new(AtomicU32::new(0));
        match open(mixer, prefs, frames.clone()) {
            Ok(o) => {
                let sr = o.sample_rate;
                let note = (sr != SR as u32).then(|| {
                    format!(
                        "出口が {sr}Hz です（{}Hz を頼みましたが通りませんでした）。\
                         再生が {:+.1}% ずれます。書き出しは正しい速さで出ます",
                        SR as u32,
                        (sr as f32 / SR - 1.0) * 100.0
                    )
                });
                Out {
                    stream: Some(o.stream),
                    error: None,
                    sample_rate: sr,
                    note,
                    host: o.host,
                    device: o.device,
                    asked: prefs.buffer,
                    frames,
                }
            }
            Err(e) => Out {
                stream: None,
                error: Some(e),
                sample_rate: SR as u32,
                note: None,
                host: prefs.host.clone(),
                device: prefs.device.clone(),
                asked: prefs.buffer,
                frames,
            },
        }
    }

    /// 実際に来た塊のうち一番大きかったもの。まだ1回も来ていなければ `None`。
    pub fn frames(&self) -> Option<u32> {
        match self.frames.load(Ordering::Relaxed) {
            0 => None,
            n => Some(n),
        }
    }

    /// その塊ぶんの遅れ（ミリ秒）。
    pub fn latency_ms(&self) -> Option<f32> {
        let n = self.frames()?;
        Some(n as f32 * 1000.0 / self.sample_rate.max(1) as f32)
    }
}

/// 使える口の名前。
pub fn hosts() -> Vec<String> {
    cpal::available_hosts().iter().map(|h| h.name().to_string()).collect()
}

/// 名前から口を引く。見つからなければ既定の口。
fn host_by_name(name: &str) -> cpal::Host {
    for id in cpal::available_hosts() {
        if id.name() == name {
            if let Ok(h) = cpal::host_from_id(id) {
                return h;
            }
        }
    }
    cpal::default_host()
}

/// その口に繋がっている機械の名前。
pub fn devices(host: &str) -> Vec<String> {
    let h = host_by_name(host);
    match h.output_devices() {
        Ok(list) => list.filter_map(|d| d.name().ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// 開けた口の中身。
struct Opened {
    stream: cpal::Stream,
    sample_rate: u32,
    host: String,
    device: String,
}

/// その形式で出せるか。小さいほど良い。出せない形式は `None`。
///
/// **順番を付けるのが肝。** 機械は形式をいくつも並べて出してきて、その
/// 先頭が出せない形式のことがある（この機械は U8 が先頭だった）。
/// 並び順で選ぶと、出せない形式を掴んで**音が一切出なくなる**
fn rank(f: cpal::SampleFormat) -> Option<u8> {
    match f {
        // 作る側が f32 なので、そのまま渡せるこれが一番良い
        cpal::SampleFormat::F32 => Some(0),
        cpal::SampleFormat::I32 => Some(1),
        cpal::SampleFormat::I16 => Some(2),
        cpal::SampleFormat::U16 => Some(3),
        cpal::SampleFormat::U8 => Some(4),
        _ => None,
    }
}

/// 48kHz で、**出せる形式**で開けられるなら、そうする。
fn pick(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    let want = cpal::SampleRate(SR as u32);
    if let Ok(list) = device.supported_output_configs() {
        // 出せる形式のうち良いものを、その中で 2ch を優先する
        let mut best: Option<(u8, bool, cpal::SupportedStreamConfigRange)> = None;
        for c in list {
            if c.min_sample_rate() > want || want > c.max_sample_rate() {
                continue;
            }
            let Some(r) = rank(c.sample_format()) else { continue };
            let key = (r, c.channels() != 2);
            if best.as_ref().map(|(br, bc, _)| key < (*br, *bc)).unwrap_or(true) {
                best = Some((key.0, key.1, c));
            }
        }
        if let Some((_, _, c)) = best {
            return Ok(c.with_sample_rate(want));
        }
    }
    device.default_output_config().map_err(|e| e.to_string())
}

/// 頼んだ塊の大きさを、機械が言う範囲へ収める。
///
/// 範囲の外を頼むと、開けることそのものが失敗する機械がある。
/// **音が出ないより、頼みどおりでないほうがいい。**
fn clamp_buffer(sup: &cpal::SupportedBufferSize, want: u32) -> Option<u32> {
    if want == 0 {
        return None;
    }
    match sup {
        cpal::SupportedBufferSize::Range { min, max } => Some(want.clamp((*min).max(1), *max)),
        cpal::SupportedBufferSize::Unknown => Some(want),
    }
}

fn open(mixer: Mixer, prefs: &Prefs, frames: Arc<AtomicU32>) -> Result<Opened, String> {
    let host = if prefs.host.is_empty() { cpal::default_host() } else { host_by_name(&prefs.host) };
    let host_name = host.id().name().to_string();

    // 名前で指定されていればそれを、無ければ既定を。
    // 指定された名前が見つからないときも、黙って既定で開ける
    // （機械を抜いただけで音が出なくなるより、鳴ったほうがいい）
    let named = (!prefs.device.is_empty())
        .then(|| {
            host.output_devices()
                .ok()?
                .find(|d| d.name().map(|n| n == prefs.device).unwrap_or(false))
        })
        .flatten();
    let device =
        named.or_else(|| host.default_output_device()).ok_or("音の出口が見つかりません")?;
    let dev_name = device.name().unwrap_or_else(|_| "（名前不明）".into());

    let sup = pick(&device)?;
    let sample_rate = sup.sample_rate().0;
    let channels = sup.channels() as usize;
    let fmt = sup.sample_format();
    let asked = clamp_buffer(sup.buffer_size(), prefs.buffer);

    let mut cfg: cpal::StreamConfig = sup.into();
    if let Some(n) = asked {
        cfg.buffer_size = cpal::BufferSize::Fixed(n);
    }

    let stream = build(&device, &cfg, fmt, channels, mixer, frames).map_err(|e| match asked {
        // **なぜ開かなかったかを、頼んだ値と一緒に言う。**
        // 「開けません」だけだと、塊を戻せば直ると気付けない
        Some(n) => format!("{e}（{n}サンプルで開けませんでした。おまかせに戻してください）"),
        None => e,
    })?;

    stream.play().map_err(|e| e.to_string())?;
    Ok(Opened { stream, sample_rate, host: host_name, device: dev_name })
}

fn build(
    device: &cpal::Device,
    cfg: &cpal::StreamConfig,
    fmt: cpal::SampleFormat,
    channels: usize,
    mut mixer: Mixer,
    seen: Arc<AtomicU32>,
) -> Result<cpal::Stream, String> {
    // 作業用の置き場。鳴らしている最中に伸ばさなくていいよう、広めに取る
    let (mut l, mut r) = (vec![0.0f32; 4096], vec![0.0f32; 4096]);
    let mut fill = move |out: &mut [f32]| {
        let frames = out.len() / channels.max(1);
        // **一番大きかった塊を覚える。** 頼んだ値ではなく、これが遅れを決める。
        //
        // 毎回同じ数だけ来るとは限らない。この機械の WASAPI は 480 と 1056 を
        // 行き来する。最後の1回を覚えると、見るたびに違う数が出て読めない。
        // 間に合わせないといけないのは一番大きい塊なので、それを出す
        seen.fetch_max(frames as u32, Ordering::Relaxed);
        if l.len() < frames {
            l.resize(frames, 0.0);
            r.resize(frames, 0.0);
        }
        mixer.fill(&mut l[..frames], &mut r[..frames]);
        for (i, frame) in out.chunks_mut(channels).enumerate() {
            let (a, b) = (l[i], r[i]);
            for (k, s) in frame.iter_mut().enumerate() {
                // 1ch なら混ぜる。3ch 以上は前2つだけ使う
                *s = match (channels, k) {
                    (1, _) => (a + b) * 0.5,
                    (_, 0) => a,
                    (_, 1) => b,
                    _ => 0.0,
                };
            }
        }
    };

    let err = |e| eprintln!("[音] {e}");
    match fmt {
        cpal::SampleFormat::F32 => {
            device.build_output_stream(cfg, move |d: &mut [f32], _: &_| fill(d), err, None)
        }
        cpal::SampleFormat::I16 => {
            let mut buf = Vec::new();
            device.build_output_stream(
                cfg,
                move |d: &mut [i16], _: &_| {
                    buf.resize(d.len(), 0.0);
                    fill(&mut buf);
                    for (o, v) in d.iter_mut().zip(&buf) {
                        *o = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                },
                err,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let mut buf = Vec::new();
            device.build_output_stream(
                cfg,
                move |d: &mut [u16], _: &_| {
                    buf.resize(d.len(), 0.0);
                    fill(&mut buf);
                    for (o, v) in d.iter_mut().zip(&buf) {
                        *o = ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 65535.0) as u16;
                    }
                },
                err,
                None,
            )
        }
        cpal::SampleFormat::I32 => {
            let mut buf = Vec::new();
            device.build_output_stream(
                cfg,
                move |d: &mut [i32], _: &_| {
                    buf.resize(d.len(), 0.0);
                    fill(&mut buf);
                    for (o, v) in d.iter_mut().zip(&buf) {
                        *o = (v.clamp(-1.0, 1.0) as f64 * 2_147_483_647.0) as i32;
                    }
                },
                err,
                None,
            )
        }
        cpal::SampleFormat::U8 => {
            let mut buf = Vec::new();
            device.build_output_stream(
                cfg,
                move |d: &mut [u8], _: &_| {
                    buf.resize(d.len(), 0.0);
                    fill(&mut buf);
                    for (o, v) in d.iter_mut().zip(&buf) {
                        *o = ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 255.0) as u8;
                    }
                },
                err,
                None,
            )
        }
        f => return Err(format!("扱えない形式です: {f:?}")),
    }
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use tonescript_engine::Engine;

    #[test]
    fn it_opens_or_says_why() {
        // 音が出せない機械でも落ちないこと。開けたかどうかは問わない
        let (_e, m) = Engine::new();
        let out = Out::start(m);
        match (&out.error, &out.stream) {
            (Some(e), None) => assert!(!e.is_empty(), "理由が空"),
            (None, Some(_)) => assert!(out.sample_rate > 0),
            _ => panic!("開けたのか開けなかったのか分からない状態"),
        }
    }

    /// **鳴る機械では、必ず鳴ること。**
    ///
    /// 前はここが「開けても開けなくても良い」だったので、形式の選び方を
    /// 間違えて音が一切出なくなっていたのに、試験は全部通っていた
    #[test]
    fn a_machine_with_a_working_device_actually_opens() {
        use cpal::traits::HostTrait;
        let host = cpal::default_host();
        let Some(dev) = host.default_output_device() else { return };
        // 既定の設定が取れる = 鳴らせる機械。ならこちらも開けなければおかしい
        if dev.default_output_config().is_err() {
            return;
        }
        let (_e, m) = Engine::new();
        let out = Out::start(m);
        assert!(out.error.is_none(), "鳴る機械なのに開けなかった: {:?}", out.error);
    }

    #[test]
    fn a_format_we_cannot_play_is_never_chosen() {
        // 作る側が f32 なので、そのまま渡せる形式が一番
        assert_eq!(rank(cpal::SampleFormat::F32), Some(0));
        // この機械は U8 を先頭に並べてきた。並び順で選ぶと音が出なくなる
        assert!(
            rank(cpal::SampleFormat::U8) > rank(cpal::SampleFormat::I16),
            "U8 が I16 より良いことになっている"
        );
        assert!(rank(cpal::SampleFormat::I16) > rank(cpal::SampleFormat::F32));
        // 扱えないものは選ばない
        assert_eq!(rank(cpal::SampleFormat::I64), None);
        assert_eq!(rank(cpal::SampleFormat::F64), None);
    }

    #[test]
    fn a_mismatched_rate_is_reported() {
        // 48kHz で開けたなら注意書きは出ない
        let (_e, m) = Engine::new();
        let out = Out::start(m);
        if out.error.is_none() && out.sample_rate == SR as u32 {
            assert!(out.note.is_none(), "合っているのに注意が出ている");
        }
    }

    #[test]
    fn there_is_always_at_least_one_host() {
        // cpal は必ず1つは返す。ここが空なら、選ぶ窓が空になってしまう
        assert!(!hosts().is_empty(), "口が1つも無い");
    }

    #[test]
    fn a_silly_buffer_is_pulled_into_range() {
        let sup = cpal::SupportedBufferSize::Range { min: 128, max: 1024 };
        assert_eq!(clamp_buffer(&sup, 16), Some(128), "小さすぎる頼みが通った");
        assert_eq!(clamp_buffer(&sup, 8192), Some(1024), "大きすぎる頼みが通った");
        assert_eq!(clamp_buffer(&sup, 256), Some(256));
        // 0 はおまかせ。機械に決めさせる
        assert_eq!(clamp_buffer(&sup, 0), None);
        // 範囲が分からない機械では、そのまま頼む
        assert_eq!(clamp_buffer(&cpal::SupportedBufferSize::Unknown, 64), Some(64));
        assert_eq!(clamp_buffer(&cpal::SupportedBufferSize::Unknown, 0), None);
    }

    #[test]
    fn the_latency_is_what_actually_came_not_what_was_asked() {
        let (_e, m) = Engine::new();
        let mut out = Out::start(m);
        // まだ1回も来ていなければ、遅れは言えない（0ms と言うのは嘘）
        out.frames.store(0, Ordering::Relaxed);
        assert_eq!(out.latency_ms(), None, "来ていないのに遅れを答えた");
        out.sample_rate = 48_000;
        out.asked = 64;
        out.frames.store(512, Ordering::Relaxed);
        let ms = out.latency_ms().unwrap();
        assert!((ms - 10.667).abs() < 0.01, "512/48k が {ms}ms になった");
    }

    /// 実際に開いて、**来た塊を数える**。
    ///
    /// 口によっては頼みを聞かない。聞かない口で「64 にしました」と出すのは
    /// 嘘なので、ここで本当の所を見ておく。音の出せない機械では何もしない。
    #[test]
    fn a_smaller_buffer_is_never_worse_than_a_bigger_one() {
        let mut got: Vec<(u32, Option<u32>)> = Vec::new();
        for want in [0u32, 64, 128, 256, 512, 1024, 2048] {
            let (_e, m) = Engine::new();
            let out = Out::open_with(m, &Prefs { buffer: want, ..Default::default() });
            if out.error.is_some() {
                eprintln!("[遅れ] {want} は開けません: {}", out.error.unwrap());
                got.push((want, None));
                continue;
            }
            // 1回でも呼ばれるまで待つ。呼ばれなければ数えようがない
            let t0 = Instant::now();
            while out.frames().is_none() && t0.elapsed() < Duration::from_millis(500) {
                std::thread::sleep(Duration::from_millis(10));
            }
            match (out.frames(), out.latency_ms()) {
                (Some(n), Some(ms)) => {
                    eprintln!("[遅れ] 頼み {want:>4} -> 実際 {n:>4} サンプル = {ms:.1}ms");
                    got.push((want, Some(n)));
                }
                _ => {
                    eprintln!("[遅れ] 頼み {want:>4} -> 呼ばれませんでした");
                    got.push((want, None));
                }
            }
        }
        let at = |want: u32| got.iter().find(|(w, _)| *w == want).and_then(|(_, n)| *n);
        if let (Some(a), Some(b)) = (at(64), at(2048)) {
            // 頼みを聞く口なら小さくなり、聞かない口なら同じ。
            // **大きくなったら、頼み方がどこか間違っている**
            //
            // この機械（WASAPI の共有）は 1056 より小さくできない。
            // 64 を頼んでも 1056 のまま、2048 を頼むと 2048 になる
            assert!(a <= b, "64 を頼んだら {a}、2048 を頼んだら {b} になった");
        }
    }

    #[test]
    fn the_sizes_offered_are_all_powers_of_two() {
        for n in Prefs::SIZES {
            assert!(n.is_power_of_two(), "{n} は2の冪ではない");
        }
        assert!(Prefs::SIZES.windows(2).all(|w| w[0] < w[1]), "小さい順に並んでいない");
    }
}
