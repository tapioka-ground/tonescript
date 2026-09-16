//! 音を鳴らす。
//!
//! 作りかた
//! --------
//! 曲まるごとを一度作って帯に持ち、そこから再生する。DAW のように
//! 「鳴らしながら計算する」形にはしていない。
//!
//! そうできるのは、作るのが速いから。34 秒の曲が 0.24 秒でできるので、
//! 音符を触ってから作り直しても間に合う。鳴らしながら計算する形にすると、
//! 音が途切れないよう締め切りを守り続ける必要があって、作りが何倍も
//! 面倒になる。今の速さなら、その面倒を買う理由がない。
//!
//! 再生の位置は「何サンプル目か」を1つの整数で持ち、音を出す側の
//! スレッドがそれを進める。画面はそれを読むだけ。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// 鳴らす中身。左右そろえて持つ。
#[derive(Default)]
pub struct Clip {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

impl Clip {
    pub fn len(&self) -> usize {
        self.l.len().min(self.r.len())
    }
}

/// 音の出口。
pub struct Player {
    clip: Arc<Mutex<Clip>>,
    /// 今どこを鳴らしているか（サンプル）
    pos: Arc<AtomicUsize>,
    playing: Arc<AtomicBool>,
    /// 繰り返す範囲（サンプル）。to が 0 なら繰り返さない
    loop_from: Arc<AtomicUsize>,
    loop_to: Arc<AtomicUsize>,
    /// 実際に鳴らしている出口。落とすと止まる
    stream: Option<cpal::Stream>,
    /// 出口を開けなかったときの理由
    pub error: Option<String>,
    /// 出口のサンプリング周波数
    pub sample_rate: u32,
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    pub fn new() -> Self {
        let clip = Arc::new(Mutex::new(Clip::default()));
        let pos = Arc::new(AtomicUsize::new(0));
        let playing = Arc::new(AtomicBool::new(false));
        let loop_from = Arc::new(AtomicUsize::new(0));
        let loop_to = Arc::new(AtomicUsize::new(0));

        let mut me = Self {
            clip: clip.clone(),
            pos: pos.clone(),
            playing: playing.clone(),
            loop_from: loop_from.clone(),
            loop_to: loop_to.clone(),
            stream: None,
            error: None,
            sample_rate: 48_000,
        };
        match open(clip, pos, playing, loop_from, loop_to) {
            Ok((s, sr)) => {
                me.stream = Some(s);
                me.sample_rate = sr;
            }
            // 音が出せなくても、編集と書き出しは続けられるようにする
            Err(e) => me.error = Some(e),
        }
        me
    }

    /// 鳴らす中身を差し替える。位置は動かさない。
    pub fn set_clip(&self, l: Vec<f32>, r: Vec<f32>) {
        if let Ok(mut c) = self.clip.lock() {
            *c = Clip { l, r };
        }
    }

    pub fn clip_len(&self) -> usize {
        self.clip.lock().map(|c| c.len()).unwrap_or(0)
    }

    pub fn has_clip(&self) -> bool {
        self.clip_len() > 0
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub fn position(&self) -> usize {
        self.pos.load(Ordering::Relaxed)
    }

    pub fn seek(&self, sample: usize) {
        self.pos.store(sample, Ordering::Relaxed);
    }

    pub fn play(&self) {
        if self.has_clip() {
            self.playing.store(true, Ordering::Relaxed);
        }
    }

    pub fn stop(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }

    /// 繰り返す範囲を決める。`to` が `from` 以下なら繰り返さない。
    pub fn set_loop(&self, from: usize, to: usize) {
        self.loop_from.store(from, Ordering::Relaxed);
        self.loop_to.store(if to > from { to } else { 0 }, Ordering::Relaxed);
    }

    pub fn loop_range(&self) -> Option<(usize, usize)> {
        let to = self.loop_to.load(Ordering::Relaxed);
        if to == 0 {
            return None;
        }
        Some((self.loop_from.load(Ordering::Relaxed), to))
    }
}

/// 音の出口を開ける。
fn open(
    clip: Arc<Mutex<Clip>>,
    pos: Arc<AtomicUsize>,
    playing: Arc<AtomicBool>,
    loop_from: Arc<AtomicUsize>,
    loop_to: Arc<AtomicUsize>,
) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("音の出口が見つかりません")?;
    let cfg = device.default_output_config().map_err(|e| e.to_string())?;
    let sample_rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;

    let fill = move |out: &mut [f32]| {
        let on = playing.load(Ordering::Relaxed);
        let Ok(c) = clip.lock() else {
            out.fill(0.0);
            return;
        };
        let n = c.len();
        if !on || n == 0 {
            out.fill(0.0);
            return;
        }
        let lo = loop_from.load(Ordering::Relaxed);
        let hi = loop_to.load(Ordering::Relaxed);
        let mut i = pos.load(Ordering::Relaxed);
        for frame in out.chunks_mut(channels) {
            // 繰り返す範囲を出たら頭へ戻る
            if hi > lo && i >= hi {
                i = lo;
            }
            if i >= n {
                // 終わりまで来た。止めて頭へは戻さない（そこから続けられる）
                playing.store(false, Ordering::Relaxed);
                for s in frame.iter_mut() {
                    *s = 0.0;
                }
                continue;
            }
            let (a, b) = (c.l[i], c.r[i]);
            for (k, s) in frame.iter_mut().enumerate() {
                // 1ch なら混ぜる。3ch 以上は前2つだけ使う
                *s = match (channels, k) {
                    (1, _) => (a + b) * 0.5,
                    (_, 0) => a,
                    (_, 1) => b,
                    _ => 0.0,
                };
            }
            i += 1;
        }
        pos.store(i, Ordering::Relaxed);
    };

    let err = |e| eprintln!("[音] {e}");
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &cfg.into(),
            move |d: &mut [f32], _: &_| fill(d),
            err,
            None,
        ),
        cpal::SampleFormat::I16 => {
            let mut buf = Vec::new();
            device.build_output_stream(
                &cfg.into(),
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
                &cfg.into(),
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
        f => return Err(format!("扱えない形式です: {f:?}")),
    }
    .map_err(|e| e.to_string())?;

    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_length_is_the_shorter_side() {
        let c = Clip { l: vec![0.0; 10], r: vec![0.0; 4] };
        assert_eq!(c.len(), 4);
        assert_eq!(Clip::default().len(), 0);
    }

    #[test]
    fn player_without_a_device_still_works() {
        // 音が出せない環境でも、落ちずに編集を続けられること。
        // ここでは出口が開くかどうかは問わない。
        let p = Player::new();
        assert!(!p.is_playing());
        assert_eq!(p.position(), 0);
        // 中身が無いうちは鳴り出さない
        p.play();
        assert!(!p.is_playing(), "空なのに再生が始まった");
        p.set_clip(vec![0.0; 100], vec![0.0; 100]);
        assert_eq!(p.clip_len(), 100);
        p.play();
        assert!(p.is_playing());
        p.stop();
        assert!(!p.is_playing());
    }

    #[test]
    fn seek_and_toggle() {
        let p = Player::new();
        p.set_clip(vec![0.0; 1000], vec![0.0; 1000]);
        p.seek(480);
        assert_eq!(p.position(), 480);
        p.play();
        assert!(p.is_playing());
        p.stop();
        assert!(!p.is_playing());
    }

    #[test]
    fn loop_range_needs_a_real_span() {
        let p = Player::new();
        assert_eq!(p.loop_range(), None);
        p.set_loop(100, 200);
        assert_eq!(p.loop_range(), Some((100, 200)));
        // 逆向きや幅ゼロは繰り返さない
        p.set_loop(200, 100);
        assert_eq!(p.loop_range(), None);
        p.set_loop(50, 50);
        assert_eq!(p.loop_range(), None);
    }
}
