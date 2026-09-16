//! 日本語のフォント。
//!
//! egui が最初から持っているフォントには日本語が入っていない。
//! そのまま出すと、画面の文字が全部「□」になる。
//!
//! フォントを binary に埋め込む手もあるが、日本語のフォントは数MBあり、
//! 配布のライセンスも font ごとに違う。ここでは OS が持っているものを
//! 借りる。Windows ならメイリオか游ゴシックが必ず入っている。
//!
//! 見つからなければ、そのことを画面に出して英数字だけで動かす。
//! 黙って豆腐だらけにするよりはいい。

use std::path::{Path, PathBuf};

/// 探す先。上から順に試す。
///
/// ttc（フォントの詰め合わせ）は中の何番目かを指す必要がある。
/// メイリオの ttc は 0 番がメイリオ、1 番がメイリオイタリック。
const CANDIDATES: &[(&str, u32)] = &[
    // --- Windows
    (r"C:\Windows\Fonts\meiryo.ttc", 0),
    (r"C:\Windows\Fonts\YuGothM.ttc", 0),
    (r"C:\Windows\Fonts\YuGothR.ttc", 0),
    (r"C:\Windows\Fonts\NotoSansJP-VF.ttf", 0),
    (r"C:\Windows\Fonts\msgothic.ttc", 0),
    // --- macOS
    ("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", 0),
    ("/Library/Fonts/Osaka.ttf", 0),
    // --- Linux
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
    ("/usr/share/fonts/truetype/fonts-japanese-gothic.ttf", 0),
];

/// 見つかった日本語フォント。
pub struct Found {
    pub path: PathBuf,
    pub index: u32,
    pub bytes: Vec<u8>,
}

/// フォントの中身として通りそうか、頭を見て確かめる。
///
/// 中を全部読むわけではないが、拡張子だけで信じるよりは確か。
/// 別のものを渡すと egui が読み込みで落ちる。
fn looks_like_font(b: &[u8], index: u32) -> bool {
    if b.len() < 12 {
        return false;
    }
    match &b[0..4] {
        // 詰め合わせ。中に何本入っているかを見て、指した番号があるか確かめる
        b"ttcf" => {
            let n = u32::from_be_bytes([b[8], b[9], b[10], b[11]]);
            index < n
        }
        // TrueType
        [0x00, 0x01, 0x00, 0x00] | b"true" => index == 0,
        // OpenType（CFF）
        b"OTTO" => index == 0,
        _ => false,
    }
}

/// OS の中から日本語フォントを1つ見つける。
pub fn find() -> Option<Found> {
    for (path, index) in CANDIDATES {
        let p = Path::new(path);
        if !p.exists() {
            continue;
        }
        let Ok(bytes) = std::fs::read(p) else { continue };
        if !looks_like_font(&bytes, *index) {
            continue;
        }
        return Some(Found { path: p.to_path_buf(), index: *index, bytes });
    }
    None
}

/// egui のフォントの並びに、日本語を足す。
///
/// 既定のフォントは消さない。英数字は元のほうが読みやすいので、
/// 先に元を置いて、無い字だけ日本語のフォントへ落ちるようにする。
pub fn install(ctx: &egui::Context) -> Result<PathBuf, String> {
    let Some(found) = find() else {
        return Err("日本語のフォントが見つかりません".into());
    };
    let mut defs = egui::FontDefinitions::default();
    let mut fd = egui::FontData::from_owned(found.bytes);
    // ttc（詰め合わせ）の何番目かを指す。既定は 0 だが、明示しておく
    fd.index = found.index;
    defs.font_data.insert("jp".to_owned(), fd);
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        defs.families.entry(family).or_default().push("jp".to_owned());
    }
    ctx.set_fonts(defs);
    Ok(found.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// その文字に本当に絵が付いているか、どの絵かを見る。
    ///
    /// 幅では分からない。全角の文字は、豆腐（□）でも本物でも同じ
    /// 1文字ぶんの幅になるため。egui は持っていない文字を
    /// 「置き換えの字」に差し替えるので、字ごとに絵が違うかで見る。
    ///
    /// 返すのは (文字, 絵が付いているか, テクスチャ上の位置)。
    fn glyphs(ctx: &egui::Context, text: &str) -> Vec<(char, bool, (u16, u16))> {
        let mut out = Vec::new();
        let _ = ctx.run(Default::default(), |ctx| {
            out = ctx.fonts(|f| {
                f.layout_no_wrap(
                    text.to_owned(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                )
                .rows
                .iter()
                .flat_map(|r| r.glyphs.iter())
                .map(|g| {
                    let u = g.uv_rect;
                    (g.chr, !u.is_nothing(), (u.min[0], u.min[1]))
                })
                .collect()
            });
        });
        out
    }

    #[test]
    fn header_check_accepts_real_fonts_only() {
        // 詰め合わせ: 2本入っているとき、0 と 1 は通り 2 は通らない
        let mut ttc = b"ttcf".to_vec();
        ttc.extend_from_slice(&[0, 1, 0, 0]);
        ttc.extend_from_slice(&2u32.to_be_bytes());
        assert!(looks_like_font(&ttc, 0));
        assert!(looks_like_font(&ttc, 1));
        assert!(!looks_like_font(&ttc, 2), "無い番号を指したのに通った");

        let ttf = [&[0x00, 0x01, 0x00, 0x00][..], &[0u8; 8][..]].concat();
        assert!(looks_like_font(&ttf, 0));
        assert!(!looks_like_font(&ttf, 1), "ttf に 1 番は無い");

        let otf = [&b"OTTO"[..], &[0u8; 8][..]].concat();
        assert!(looks_like_font(&otf, 0));

        assert!(!looks_like_font("これはフォントではない".as_bytes(), 0));
        assert!(!looks_like_font(&[], 0));
        assert!(!looks_like_font(b"PKaaaaaaaa", 0));
    }

    #[test]
    fn a_japanese_font_is_found_on_this_machine() {
        let f = find().expect("日本語フォントが1つも見つからない");
        assert!(f.bytes.len() > 100_000, "小さすぎる: {} バイト", f.bytes.len());
        assert!(looks_like_font(&f.bytes, f.index));
        println!(
            "見つかった: {} ({}番, {:.1} MB)",
            f.path.display(),
            f.index,
            f.bytes.len() as f64 / 1048576.0
        );
    }

    #[test]
    fn without_a_japanese_font_everything_is_tofu() {
        // 前提の確認。何も入れなければ日本語は出ない。
        // ここが変わったら、egui 側が日本語を持つようになったということ。
        let ctx = egui::Context::default();
        let g = glyphs(&ctx, "日本語");
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].2, g[1].2, "字が違うのに同じ絵＝豆腐のはず");
        assert_eq!(g[1].2, g[2].2);
    }

    /// 画面を出さずに、実際に字を組んでみて日本語が出るか確かめる。
    /// ここが通らないと、画面の文字が全部「□」になる。
    #[test]
    fn japanese_actually_renders() {
        let ctx = egui::Context::default();
        let path = install(&ctx).expect("日本語フォントを入れられない");
        println!("使ったフォント: {}", path.display());

        let g = glyphs(&ctx, "日本語");
        assert!(g.iter().all(|x| x.1), "絵が付いていない字がある");
        assert_ne!(g[0].2, g[1].2, "日 と 本 が同じ絵。豆腐になっている");
        assert_ne!(g[1].2, g[2].2, "本 と 語 が同じ絵。豆腐になっている");

        // 画面で実際に使う文字を一通り
        for s in [
            "小節", "拍子", "取り消す", "やり直す", "自動保存", "主旋律",
            "サビ", "書き出す", "ベース", "アルペジオ", "パーカス",
            "前回きちんと閉じていません", "曲を選んでください",
        ] {
            let g = glyphs(&ctx, s);
            assert!(g.iter().all(|x| x.1), "絵が出ない: {s}");
            assert!(
                g.iter().any(|x| x.2 != g[0].2),
                "全部同じ絵になっている（豆腐）: {s}"
            );
        }

        // 英数字が壊れていないこと
        let a = glyphs(&ctx, "Tonescript 128 BPM");
        assert!(a.iter().all(|x| x.1 || x.0 == ' '), "英数字が出ない");
    }

    #[test]
    fn every_candidate_on_this_machine_works() {
        // 見つかる候補は、どれを使っても日本語が出ること。
        let mut tried = 0;
        for (path, index) in CANDIDATES {
            if !Path::new(path).exists() {
                continue;
            }
            let Ok(bytes) = std::fs::read(path) else { continue };
            if !looks_like_font(&bytes, *index) {
                continue;
            }
            let ctx = egui::Context::default();
            let mut defs = egui::FontDefinitions::default();
            let mut fd = egui::FontData::from_owned(bytes);
            fd.index = *index;
            defs.font_data.insert("jp".to_owned(), fd);
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                defs.families.entry(fam).or_default().push("jp".to_owned());
            }
            ctx.set_fonts(defs);
            let g = glyphs(&ctx, "日本語");
            assert_ne!(g[0].2, g[1].2, "豆腐になる: {path}");
            tried += 1;
        }
        assert!(tried > 0, "この PC に日本語フォントが1つも無い");
        println!("{tried} 本の候補が使える");
    }
}
