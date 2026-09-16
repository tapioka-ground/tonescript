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

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tonescript_dsp::osc::SR;
use tonescript_engine::Mixer;

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
}

impl Out {
    /// 音の出口を開けて、ミキサーを繋ぐ。
    pub fn start(mixer: Mixer) -> Out {
        match open(mixer) {
            Ok((stream, sr)) => {
                let note = (sr != SR as u32).then(|| {
                    format!(
                        "出口が {sr}Hz です（{}Hz を頼みましたが通りませんでした）。\
                         再生が {:+.1}% ずれます。書き出しは正しい速さで出ます",
                        SR as u32,
                        (sr as f32 / SR - 1.0) * 100.0
                    )
                });
                Out { stream: Some(stream), error: None, sample_rate: sr, note }
            }
            Err(e) => Out { stream: None, error: Some(e), sample_rate: SR as u32, note: None },
        }
    }
}

/// 48kHz で開けられるなら、そうする。
fn pick(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, String> {
    let want = cpal::SampleRate(SR as u32);
    if let Ok(list) = device.supported_output_configs() {
        // 2ch を優先する。無ければ何でも
        let mut best: Option<cpal::SupportedStreamConfigRange> = None;
        for c in list {
            if c.min_sample_rate() <= want && want <= c.max_sample_rate() {
                let better = match &best {
                    None => true,
                    Some(b) => c.channels() == 2 && b.channels() != 2,
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
    device.default_output_config().map_err(|e| e.to_string())
}

fn open(mut mixer: Mixer) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("音の出口が見つかりません")?;
    let cfg = pick(&device)?;
    let sample_rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;

    // 作業用の置き場。鳴らしている最中に伸ばさなくていいよう、広めに取る
    let (mut l, mut r) = (vec![0.0f32; 4096], vec![0.0f32; 4096]);
    let mut fill = move |out: &mut [f32]| {
        let frames = out.len() / channels.max(1);
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

    #[test]
    fn a_mismatched_rate_is_reported() {
        // 48kHz で開けたなら注意書きは出ない
        let (_e, m) = Engine::new();
        let out = Out::start(m);
        if out.error.is_none() && out.sample_rate == SR as u32 {
            assert!(out.note.is_none(), "合っているのに注意が出ている");
        }
    }
}
