//! WAV の読み書き。
//!
//! 外のライブラリを入れていない。WAV は素直な形式なので、自分で書いた
//! ほうが依存が減る。「軽くしたい」がこの企画の目的の一つ。
//!
//! 書けるのは 16bit / 24bit の整数と 32bit の小数。深さと周波数の選び方は
//! [`crate::export`] を見よ。

use std::io::{self, Read, Write};
use std::path::Path;

use crate::mix::Stereo;

/// 16bit ステレオで書き出す。
pub fn write_stereo(path: &Path, s: &Stereo, sample_rate: u32) -> io::Result<()> {
    write_stereo_as(path, s, sample_rate, crate::export::Depth::I16, true)
}

/// 深さを選んで書き出す。
///
/// `dither` は 16bit へ落とすときだけ効く（[`crate::export::Dither`]）。
pub fn write_stereo_as(
    path: &Path,
    s: &Stereo,
    sample_rate: u32,
    depth: crate::export::Depth,
    dither: bool,
) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let frames = s.l.len().min(s.r.len());
    let mut f = io::BufWriter::new(std::fs::File::create(path)?);
    write_header(&mut f, frames as u32, 2, sample_rate, depth)?;
    let mut d = dither.then(|| crate::export::Dither::new(depth)).flatten();
    for i in 0..frames {
        let n = d.as_mut().map(|d| (d.run(), d.run())).unwrap_or((0.0, 0.0));
        write_sample(&mut f, s.l[i] + n.0, depth)?;
        write_sample(&mut f, s.r[i] + n.1, depth)?;
    }
    f.flush()
}

/// 16bit モノラルで書き出す。パート別の書き出し用。
pub fn write_mono(path: &Path, x: &[f32], sample_rate: u32) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = io::BufWriter::new(std::fs::File::create(path)?);
    write_header(&mut f, x.len() as u32, 1, sample_rate, crate::export::Depth::I16)?;
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

fn write_sample<W: Write>(
    w: &mut W,
    v: f32,
    depth: crate::export::Depth,
) -> io::Result<()> {
    use crate::export::Depth;
    match depth {
        Depth::I16 => w.write_all(&to_i16(v).to_le_bytes()),
        Depth::I24 => {
            let x = (v.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
            w.write_all(&x.to_le_bytes()[..3])
        }
        // 小数のまま。天井を超えたぶんも残る（あとで直せる）
        Depth::F32 => w.write_all(&v.to_le_bytes()),
    }
}

fn write_header<W: Write>(
    w: &mut W,
    frames: u32,
    ch: u16,
    sr: u32,
    depth: crate::export::Depth,
) -> io::Result<()> {
    let bits = depth.bits();
    let float = depth == crate::export::Depth::F32;
    let block = ch * bits / 8;
    let data_bytes = frames * block as u32;
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // fmt の大きさ
    // 1 = 整数、3 = 小数。ここを間違えると開いた側が爆音で鳴らす
    w.write_all(&if float { 3u16 } else { 1u16 }.to_le_bytes())?;
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
    let (mut ch, mut sr, mut bits, mut tag) = (0u16, 0u32, 0u16, 1u16);
    let mut data: Option<(usize, usize)> = None;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let size = u32::from_le_bytes([b[pos + 4], b[pos + 5], b[pos + 6], b[pos + 7]]) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= b.len() {
            tag = u16::from_le_bytes([b[body], b[body + 1]]);
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
    // 自分で書き出した 24bit や 32bit の小数も、そのまま読み込めること
    let bytes = match (tag, bits) {
        (1, 16) => 2usize,
        (1, 24) => 3,
        (3, 32) => 4,
        (1, 32) => 4, // 32bit の整数
        _ => {
            return Err(format!(
                "{}: 読めない形です（{}bit / 形式 {tag}）。16 / 24 / 32bit に",
                path.display(),
                bits
            ))
        }
    };
    let stride = bytes * ch as usize;
    let frames = len / stride.max(1);
    let mut l = Vec::with_capacity(frames);
    let mut r = Vec::with_capacity(frames);
    let at = |o: usize| -> f32 {
        match (tag, bits) {
            (1, 16) => i16::from_le_bytes([b[o], b[o + 1]]) as f32 / 32768.0,
            (1, 24) => {
                // 24bit は符号を自分で伸ばす。上位へ寄せて割る
                let x = ((b[o] as i32) << 8) | ((b[o + 1] as i32) << 16) | ((b[o + 2] as i32) << 24);
                x as f32 / 2_147_483_648.0
            }
            (3, 32) => f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]),
            _ => {
                i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f32 / 2_147_483_648.0
            }
        }
    };
    for i in 0..frames {
        let o = start + i * stride;
        let a = at(o);
        l.push(a);
        r.push(if ch >= 2 { at(o + bytes) } else { a });
    }
    Ok((l, r, sr))
}

#[cfg(test)]
mod extra_tests {
    use super::*;

    #[test]
    fn every_depth_comes_back_the_same() {
        // 書いたものを読み直して、元の形が残っていること。
        // ここが合っていないと、書き出した歌を自分で読み込めない
        use crate::export::Depth;
        let n = 500;
        let src: Vec<f32> = (0..n)
            .map(|i| 0.8 * (i as f32 * std::f32::consts::TAU * 440.0 / 48_000.0).sin())
            .collect();
        let s = Stereo { l: src.clone(), r: src.iter().map(|v| -v).collect() };
        for (depth, slack) in
            [(Depth::I16, 2e-4f32), (Depth::I24, 2e-5), (Depth::F32, 1e-7)]
        {
            let mut p = std::env::temp_dir();
            p.push(format!("ts_wav_{}_{}.wav", depth.bits(), std::process::id()));
            // 粉は足さずに書く。足すと比べられない
            write_stereo_as(&p, &s, 48_000, depth, false).expect("書けるはず");
            let (l, r, sr) = read(&p).expect("読めるはず");
            assert_eq!(sr, 48_000);
            assert_eq!(l.len(), n, "{}bit で長さが変わった", depth.bits());
            for i in 0..n {
                assert!(
                    (l[i] - s.l[i]).abs() < slack,
                    "{}bit の左 {i} 番目が {} → {}",
                    depth.bits(),
                    s.l[i],
                    l[i]
                );
                assert!((r[i] - s.r[i]).abs() < slack, "{}bit の右がずれた", depth.bits());
            }
            std::fs::remove_file(&p).ok();
        }
    }

    #[test]
    fn dither_changes_the_bits_but_not_the_sound() {
        use crate::export::Depth;
        let n = 2000;
        let src: Vec<f32> = (0..n).map(|i| 0.5 * (i as f32 * 0.01).sin()).collect();
        let s = Stereo { l: src.clone(), r: src };
        let mut a = std::env::temp_dir();
        a.push(format!("ts_dither_on_{}.wav", std::process::id()));
        let mut b = std::env::temp_dir();
        b.push(format!("ts_dither_off_{}.wav", std::process::id()));
        write_stereo_as(&a, &s, 48_000, Depth::I16, true).unwrap();
        write_stereo_as(&b, &s, 48_000, Depth::I16, false).unwrap();
        let (la, _, _) = read(&a).unwrap();
        let (lb, _, _) = read(&b).unwrap();
        // 粉のぶんだけ違うが、その差は 16bit の 1〜2段に収まること
        let diff = la
            .iter()
            .zip(&lb)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        assert!(diff > 0.0, "粉を足したのに1ビットも変わっていない");
        assert!(diff < 3.0 / 32768.0, "粉が大きすぎる: {diff}");
        std::fs::remove_file(&a).ok();
        std::fs::remove_file(&b).ok();
    }

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
