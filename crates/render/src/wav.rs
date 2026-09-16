//! WAV の読み書き。
//!
//! 外のライブラリを入れていない。WAV は素直な形式で、必要なのは
//! 16bit PCM だけなので、自分で書いたほうが依存が減る。
//! 「軽くしたい」がこの企画の目的の一つなので、ここは自前にする。

use std::io::{self, Read, Write};
use std::path::Path;

use crate::mix::Stereo;

/// 16bit ステレオで書き出す。
pub fn write_stereo(path: &Path, s: &Stereo, sample_rate: u32) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let frames = s.l.len().min(s.r.len());
    let mut f = io::BufWriter::new(std::fs::File::create(path)?);
    write_header(&mut f, frames as u32, 2, sample_rate)?;
    for i in 0..frames {
        f.write_all(&to_i16(s.l[i]).to_le_bytes())?;
        f.write_all(&to_i16(s.r[i]).to_le_bytes())?;
    }
    f.flush()
}

/// 16bit モノラルで書き出す。パート別の書き出し用。
pub fn write_mono(path: &Path, x: &[f32], sample_rate: u32) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = io::BufWriter::new(std::fs::File::create(path)?);
    write_header(&mut f, x.len() as u32, 1, sample_rate)?;
    for v in x {
        f.write_all(&to_i16(*v).to_le_bytes())?;
    }
    f.flush()
}

#[inline]
fn to_i16(v: f32) -> i16 {
    // 天井で折り返さずに頭打ちにする。折り返すと「バリッ」と割れる
    (v.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

fn write_header<W: Write>(w: &mut W, frames: u32, ch: u16, sr: u32) -> io::Result<()> {
    let bits = 16u16;
    let block = ch * bits / 8;
    let data_bytes = frames * block as u32;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // fmt の大きさ
    w.write_all(&1u16.to_le_bytes())?; // 1 = PCM
    w.write_all(&ch.to_le_bytes())?;
    w.write_all(&sr.to_le_bytes())?;
    w.write_all(&(sr * block as u32).to_le_bytes())?;
    w.write_all(&block.to_le_bytes())?;
    w.write_all(&bits.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    Ok(())
}

/// 読む。歌の WAV（AUDIO_TRACKS）を取り込むのに使う。
/// 返すのは (左, 右, サンプリング周波数)。モノラルなら左右同じ。
pub fn read(path: &Path) -> Result<(Vec<f32>, Vec<f32>, u32), String> {
    let mut b = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_end(&mut b))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(format!("{}: WAV に見えません", path.display()));
    }
    let mut pos = 12;
    let (mut ch, mut sr, mut bits) = (0u16, 0u32, 0u16);
    let mut data: Option<(usize, usize)> = None;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let size = u32::from_le_bytes([b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]]) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= b.len() {
            ch = u16::from_le_bytes([b[body + 2], b[body + 3]]);
            sr = u32::from_le_bytes([b[body + 4], b[body + 5], b[body + 6], b[body + 7]]);
            bits = u16::from_le_bytes([b[body + 14], b[body + 15]]);
        } else if id == b"data" {
            data = Some((body, size.min(b.len().saturating_sub(body))));
        }
        // チャンクは偶数境界に揃う
        pos = body + size + (size & 1);
    }
    let (start, len) = data.ok_or_else(|| format!("{}: data が無い", path.display()))?;
    if ch == 0 {
        return Err(format!("{}: fmt が無い", path.display()));
    }
    if bits != 16 {
        return Err(format!("{}: 16bit だけ読めます（今は {bits}bit）", path.display()));
    }
    let frames = len / (2 * ch as usize);
    let mut l = Vec::with_capacity(frames);
    let mut r = Vec::with_capacity(frames);
    for i in 0..frames {
        let o = start + i * 2 * ch as usize;
        let a = i16::from_le_bytes([b[o], b[o + 1]]) as f32 / 32768.0;
        l.push(a);
        if ch >= 2 {
            r.push(i16::from_le_bytes([b[o + 2], b[o + 3]]) as f32 / 32768.0);
        } else {
            r.push(a);
        }
    }
    Ok((l, r, sr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("tonescript_test_{name}.wav"));
        p
    }

    #[test]
    fn stereo_round_trip() {
        let n = 1000;
        let l: Vec<f32> = (0..n).map(|i| (i as f32 / 100.0).sin() * 0.5).collect();
        let r: Vec<f32> = l.iter().map(|v| -v).collect();
        let p = tmp("stereo");
        write_stereo(&p, &Stereo { l: l.clone(), r: r.clone() }, 48_000).unwrap();
        let (gl, gr, sr) = read(&p).unwrap();
        assert_eq!(sr, 48_000);
        assert_eq!(gl.len(), n);
        for i in 0..n {
            assert!((gl[i] - l[i]).abs() < 1e-4, "左 {i}");
            assert!((gr[i] - r[i]).abs() < 1e-4, "右 {i}");
        }
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn mono_reads_back_on_both_sides() {
        let x: Vec<f32> = (0..500).map(|i| (i as f32 / 50.0).sin() * 0.3).collect();
        let p = tmp("mono");
        write_mono(&p, &x, 44_100).unwrap();
        let (l, r, sr) = read(&p).unwrap();
        assert_eq!(sr, 44_100);
        assert_eq!(l, r, "モノラルは左右同じになるはず");
        assert!((l[10] - x[10]).abs() < 1e-4);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn clipping_is_flat_not_wrapped() {
        // 天井を超えた値が折り返さないこと。折り返すと「バリッ」と割れる
        let p = tmp("clip");
        write_stereo(&p, &Stereo { l: vec![5.0, -5.0], r: vec![5.0, -5.0] }, 48_000).unwrap();
        let (l, _, _) = read(&p).unwrap();
        assert!(l[0] > 0.99 && l[1] < -0.99, "折り返した: {:?}", l);
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn garbage_is_rejected() {
        let p = tmp("garbage");
        std::fs::write(&p, "これは WAV ではない".as_bytes()).unwrap();
        assert!(read(&p).is_err());
        std::fs::remove_file(&p).ok();
    }
}
