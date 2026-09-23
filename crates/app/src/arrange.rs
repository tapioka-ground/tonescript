//! アレンジビュー。**曲を1画面で見る所。**
//!
//! ピアノロールは音符1つ1つを見る所で、その分だけ視野が狭い。曲がどんな
//! 形をしているか（どこが静かで、どこで全部鳴って、サビが何小節あるか）は、
//! 縦に音程、横に16分目盛りの画面では見えない。
//!
//! ここは逆で、**縦にパート、横に小節**。1小節が四角1つになる。
//!
//! 形
//! --
//! 左がパートの見出し、右が時間。DAW はどれもこの形をしていて、
//! **左で「何の楽器か」、右で「いつ鳴るか」**を見る。
//!
//! 見出しは背が高い。名前だけなら1行で足りるが、そこに音量とミュートを
//! 置くと、鳴らしながら釣り合いを取るのにミキサーを開かなくて済む。
//! パートを触るのが一番多い操作なので、そこを一番広く取る。
//!
//! 何を触れるか
//! ------------
//! 四角を押すと、その小節でそのパートが鳴るかどうかが変わる。曲ファイルの
//! `ARRANGE` を上書きする形で、触っていない小節は曲ファイルのまま。
//! **引きずれば塗れる**（太鼓の打ち込み機と同じ）。
//!
//! 区間（`SECTIONS`）はここでは触らない
//! ------------------------------------
//! 上の帯に出すのは見るためと、頭出し・繰り返しのため。長さや並びを
//! 変えるのは「曲の設定」に置いてある。
//!
//! この形式では旋律も和音も**小節番号で覚えている**。区間を動かしても
//! 音符は付いてこないので、ここで並べ替えられるようにすると、見た目の
//! 帯だけが動いて中身が残る、という分かりにくいことになる。

use egui::{Color32, Rect, Sense, Ui, Vec2};
use tonescript_project::Project;
use tonescript_song::Song;

use crate::theme;

/// 左のパート見出しの幅。
const LABEL_W: f32 = 196.0;
/// 区間の帯の高さ。
const HEAD_H: f32 = 24.0;
/// 小節番号の目盛りの高さ。
const RULER_H: f32 = 16.0;
/// パート1本ぶんの高さ。
///
/// **ここを詰めない。** 名前・絵・音量・ミュートを1枚に収めるには
/// これくらい要る。行が薄いと、曲の形は見えても楽器を触れない
const ROW_H: f32 = 56.0;
/// 楽器の絵の大きさ。
const ICON: f32 = 34.0;
/// 1小節の横幅。
pub const BAR_W: [f32; 5] = [8.0, 14.0, 22.0, 34.0, 52.0];

/// 触られたこと。呼ぶ側が当てる。
#[derive(Default)]
pub struct Touched {
    /// 再生ヘッドをここへ（目盛り）
    pub seek_to: Option<f32>,
    /// 繰り返す範囲（目盛り）
    pub loop_set: Option<(f32, f32)>,
    /// 繰り返しを解く
    pub loop_clear: bool,
    /// `(小節, パート, 鳴らすか)`。引きずったぶんまとめて入る
    pub plays: Vec<(u32, String, bool)>,
    /// 音量を変えた
    pub gain: Option<(String, f32)>,
    /// 黙らせる／戻す
    pub mute: Option<String>,
    /// これだけ鳴らす／戻す
    pub solo: Option<String>,
    /// 編集するパートを選んだ
    pub pick: Option<String>,
}

/// 塗っている最中の覚え書き。
#[derive(Default, Clone, Copy, PartialEq)]
pub struct Paint {
    /// 押したときに決めた「これから塗る値」
    pub on: bool,
    /// 塗っている最中か
    pub live: bool,
}

/// 目盛りを「小節いくつぶん」へ。小節の途中は小数で返す。
///
/// 拍子が変わる曲があるので、小節の幅は一定ではない。**必ず境目の表を
/// 通す**（16で割ると、変拍子の曲で後ろへ行くほどずれる）
pub fn bar_pos(starts: &[u32], step: f32) -> f32 {
    if starts.len() < 2 {
        return 0.0;
    }
    let s = step.max(0.0);
    // step がどの小節に入るか
    let i = match starts.binary_search(&(s as u32)) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    let i = i.min(starts.len() - 2);
    let (a, b) = (starts[i] as f32, starts[i + 1] as f32);
    let span = (b - a).max(1.0);
    i as f32 + ((s - a) / span).clamp(0.0, 1.0)
}

/// 逆。小節いくつぶんを目盛りへ。
pub fn step_at(starts: &[u32], pos: f32) -> f32 {
    if starts.len() < 2 {
        return 0.0;
    }
    let p = pos.max(0.0);
    let i = (p as usize).min(starts.len() - 2);
    let (a, b) = (starts[i] as f32, starts[i + 1] as f32);
    a + (p - i as f32).clamp(0.0, 1.0) * (b - a)
}

/// 画面に出すパートの名前。曲が `label` を書いていればそちら。
fn shown_name(song: &Song, part: &str) -> String {
    song.voices
        .get(part)
        .map(|v| v.label.clone())
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| part.to_string())
}

/// 楽器の絵の代わり。
///
/// **まだ絵が無い。** 無いなら無いと書く。ここを空けておくと、
/// 描き忘れなのか、そういう見た目なのかが分からない
fn icon_box(ui: &Ui, rect: Rect, color: Color32) {
    let p = ui.painter();
    p.rect_filled(rect, 5.0, theme::PANEL);
    p.rect_stroke(rect, 5.0, (1.0, color.gamma_multiply(0.7)));
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "NO\nIMAGE",
        egui::FontId::proportional(7.5),
        theme::DIM,
    );
}

/// 左のパート見出しを1本描く。
fn track_header(
    ui: &mut Ui,
    song: &Song,
    project: &Project,
    part: &str,
    rect: Rect,
    picked: bool,
    out: &mut Touched,
) {
    let color = theme::part_color(part);
    let muted = project.muted.iter().any(|m| m == part);
    let soloed = project.soloed.iter().any(|m| m == part);

    {
        let p = ui.painter();
        // 選んでいるパートは明るく。ピアノロールで触るのがこれ
        let bg = if picked { Color32::from_gray(38) } else { Color32::from_gray(28) };
        let body = Rect::from_min_max(rect.min, rect.max - Vec2::new(0.0, 1.0));
        p.rect_filled(body, 4.0, bg);
        // 左端の色の帯。ピアノロールの音符と同じ色
        p.rect_filled(
            Rect::from_min_size(body.min, Vec2::new(3.0, body.height())),
            2.0,
            if muted { color.gamma_multiply(0.3) } else { color },
        );
    }

    let x = rect.left() + 9.0;
    let y = rect.top() + 3.0;
    icon_box(ui, Rect::from_min_size(egui::pos2(x, y + 7.0), Vec2::splat(ICON)), color);

    // 名前。押すと編集するパートが変わる
    let name_r = Rect::from_min_size(
        egui::pos2(x + ICON + 8.0, y + 4.0),
        Vec2::new(rect.right() - x - ICON - 16.0, 18.0),
    );
    let label = shown_name(song, part);
    let text = if label == part { label.clone() } else { format!("{label}  ({part})") };
    if ui
        .put(name_r, egui::SelectableLabel::new(picked, text))
        .on_hover_text("押すとピアノロールで触るパートが変わります")
        .clicked()
    {
        out.pick = Some(part.to_string());
    }

    // 黙らせる・これだけ鳴らす
    let by = y + 28.0;
    let mx = x + ICON + 8.0;
    if ui
        .put(
            Rect::from_min_size(egui::pos2(mx, by), Vec2::new(19.0, 18.0)),
            egui::SelectableLabel::new(muted, "M"),
        )
        .on_hover_text("黙らせる")
        .clicked()
    {
        out.mute = Some(part.to_string());
    }
    if ui
        .put(
            Rect::from_min_size(egui::pos2(mx + 21.0, by), Vec2::new(19.0, 18.0)),
            egui::SelectableLabel::new(soloed, "S"),
        )
        .on_hover_text("これだけ鳴らす")
        .clicked()
    {
        out.solo = Some(part.to_string());
    }

    // 音量。**ここに置くのが肝。** ミキサーを開かずに釣り合いを取れる
    let mut gain = project
        .gains
        .get(part)
        .copied()
        .unwrap_or_else(|| song.gains.get(part).copied().unwrap_or(1.0));
    let sx = mx + 46.0;
    let sw = (rect.right() - sx - 9.0).max(40.0);
    let r = ui.put(
        Rect::from_min_size(egui::pos2(sx, by), Vec2::new(sw, 18.0)),
        egui::Slider::new(&mut gain, 0.0..=4.0).show_value(false).step_by(0.01),
    );
    if r.changed() {
        out.gain = Some((part.to_string(), gain));
    }
    r.on_hover_text(format!("音量 {gain:.2}"));
}

/// 右の時間軸を描く。
fn timeline(
    ui: &mut Ui,
    song: &Song,
    project: &Project,
    head: f32,
    loop_range: Option<(f32, f32)>,
    bar_w: f32,
    paint: &mut Paint,
    out: &mut Touched,
) {
    let starts = song.bar_starts();
    let bars = song.bars();
    let parts: Vec<String> = song.edit_parts.clone();
    let h = HEAD_H + RULER_H + ROW_H * parts.len() as f32;
    let w = bar_w * bars as f32 + 8.0;

    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(w.max(ui.available_width()), h),
        Sense::click_and_drag(),
    );
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme::PANEL);

    let x_of = |bar: f32| rect.left() + bar * bar_w;
    let head_top = rect.top();
    let ruler_top = head_top + HEAD_H;
    let grid_top = ruler_top + RULER_H;

    // -- 区間の帯
    let mut at = 0u32;
    for (i, sec) in song.sections.iter().enumerate() {
        let r = Rect::from_min_max(
            egui::pos2(x_of(at as f32) + 1.0, head_top + 1.0),
            egui::pos2(x_of((at + sec.bars) as f32) - 1.0, head_top + HEAD_H - 3.0),
        );
        p.rect_filled(r, 3.0, theme::section_color(i));
        if r.width() > 24.0 {
            p.text(
                egui::pos2(r.left() + 5.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                &sec.name,
                egui::FontId::proportional(11.0),
                Color32::from_gray(20),
            );
        }
        at += sec.bars;
    }

    // -- 小節番号と縦線
    let every = if bar_w >= 34.0 {
        1
    } else if bar_w >= 14.0 {
        4
    } else {
        8
    };
    for bar in 0..bars {
        let x = x_of(bar as f32);
        let n = bar + 1;
        if n % every == 1 || every == 1 {
            p.text(
                egui::pos2(x + 3.0, ruler_top + RULER_H * 0.5),
                egui::Align2::LEFT_CENTER,
                format!("{n}"),
                egui::FontId::proportional(10.0),
                theme::DIM,
            );
        }
        // 区間の変わり目に縦線。曲の切れ目が縦に通って見える
        if song.sections.iter().scan(0u32, |a, s| {
            *a += s.bars;
            Some(*a)
        }).any(|e| e == bar) {
            p.line_segment(
                [egui::pos2(x, ruler_top), egui::pos2(x, rect.bottom())],
                (1.0, Color32::from_gray(64)),
            );
        }
    }

    // -- 繰り返している範囲
    if let Some((a, b)) = loop_range {
        p.rect_filled(
            Rect::from_min_max(
                egui::pos2(x_of(bar_pos(&starts, a)), ruler_top),
                egui::pos2(x_of(bar_pos(&starts, b)), rect.bottom()),
            ),
            0.0,
            Color32::from_rgba_unmultiplied(0x0a, 0x84, 0xff, 26),
        );
    }

    // -- パートの行
    for (row, part) in parts.iter().enumerate() {
        let y = grid_top + ROW_H * row as f32;
        let color = theme::part_color(part);
        let quiet = !project.audible(part);
        // 1行おきに薄く敷く。横に目で追うときに行を見失わない
        if row % 2 == 1 {
            p.rect_filled(
                Rect::from_min_size(
                    egui::pos2(rect.left(), y),
                    Vec2::new(rect.width(), ROW_H),
                ),
                0.0,
                Color32::from_gray(24),
            );
        }
        for bar in 0..bars {
            let n = bar + 1;
            if !project.plays(n, part, song.plays(n, part)) {
                continue;
            }
            let cell = Rect::from_min_max(
                egui::pos2(x_of(bar as f32) + 1.0, y + 5.0),
                egui::pos2(x_of((bar + 1) as f32) - 1.0, y + ROW_H - 6.0),
            );
            let c = if quiet { color.gamma_multiply(0.25) } else { color };
            p.rect_filled(cell, 3.0, c);
        }
    }

    // -- 再生ヘッド
    let hx = x_of(bar_pos(&starts, head));
    p.line_segment(
        [egui::pos2(hx, ruler_top), egui::pos2(hx, rect.bottom())],
        (1.5, theme::HEAD_ON),
    );

    // -- 触られた所
    if let Some(pt) = resp.interact_pointer_pos() {
        let bar_f = ((pt.x - rect.left()) / bar_w).max(0.0);
        let bar = (bar_f as u32).min(bars.saturating_sub(1));
        if pt.y < grid_top {
            // 上半分（区間の帯と目盛り）は頭出し。押している間ついてくる
            out.seek_to = Some(step_at(&starts, bar_f));
            // 区間を二度押しで、その区間を繰り返す
            if resp.double_clicked() && pt.y < ruler_top {
                let mut at = 0u32;
                for sec in &song.sections {
                    if bar >= at && bar < at + sec.bars {
                        let a = starts.get(at as usize).copied().unwrap_or(0) as f32;
                        let b = starts.get((at + sec.bars) as usize).copied().unwrap_or(0) as f32;
                        out.loop_set = Some((a, b));
                        out.seek_to = Some(a);
                        break;
                    }
                    at += sec.bars;
                }
            }
        } else {
            let row = ((pt.y - grid_top) / ROW_H) as usize;
            if let Some(part) = parts.get(row) {
                let n = bar + 1;
                let now = project.plays(n, part, song.plays(n, part));
                // 押した1つ目で「これから塗る値」を決める。引きずっている
                // 間はその値で塗る。1つずつ反転させると、行きと戻りで
                // 打ち消し合って塗れない
                if !paint.live {
                    paint.live = true;
                    paint.on = !now;
                }
                if now != paint.on {
                    out.plays.push((n, part.clone(), paint.on));
                }
            }
        }
    }
    if !resp.is_pointer_button_down_on() {
        paint.live = false;
    }
}

/// 描く。
pub fn panel(
    ui: &mut Ui,
    song: &Song,
    project: &Project,
    head: f32,
    loop_range: Option<(f32, f32)>,
    bar_w: f32,
    paint: &mut Paint,
    picked: &str,
) -> Touched {
    let mut out = Touched::default();
    let parts: Vec<String> = song.edit_parts.clone();
    if song.bars() == 0 || parts.is_empty() {
        ui.label(theme::dim("小節がありません"));
        return out;
    }

    ui.horizontal_top(|ui| {
        // -- 左。パートの見出し
        let h = HEAD_H + RULER_H + ROW_H * parts.len() as f32;
        let (col, _) = ui.allocate_exact_size(Vec2::new(LABEL_W, h), Sense::hover());
        ui.painter_at(col).rect_filled(col, 0.0, Color32::from_gray(20));
        for (row, part) in parts.iter().enumerate() {
            let r = Rect::from_min_size(
                egui::pos2(col.left(), col.top() + HEAD_H + RULER_H + ROW_H * row as f32),
                Vec2::new(LABEL_W, ROW_H),
            );
            track_header(ui, song, project, part, r, picked == part, &mut out);
        }

        // -- 右。時間
        egui::ScrollArea::horizontal().show(ui, |ui| {
            timeline(ui, song, project, head, loop_range, bar_w, paint, &mut out);
        });
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 4/4 が2小節、3/4 が2小節。
    fn starts() -> Vec<u32> {
        vec![0, 16, 32, 44, 56]
    }

    #[test]
    fn a_step_lands_in_the_right_bar() {
        let s = starts();
        assert_eq!(bar_pos(&s, 0.0), 0.0);
        assert_eq!(bar_pos(&s, 16.0), 1.0);
        assert_eq!(bar_pos(&s, 32.0), 2.0);
        // 変拍子の小節。12目盛りで1小節なので、6目盛りで真ん中
        assert!((bar_pos(&s, 38.0) - 2.5).abs() < 1e-5, "{}", bar_pos(&s, 38.0));
        // 4/4 の小節の真ん中は8目盛り
        assert!((bar_pos(&s, 8.0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn going_back_and_forth_lands_where_it_started() {
        let s = starts();
        for step in [0.0f32, 8.0, 16.0, 33.0, 38.0, 50.0] {
            let back = step_at(&s, bar_pos(&s, step));
            assert!((back - step).abs() < 0.01, "{step} が {back} になった");
        }
    }

    #[test]
    fn past_the_end_does_not_run_away() {
        let s = starts();
        // 曲の終わりより後ろを指されても、最後の小節までで止まる
        let p = bar_pos(&s, 9999.0);
        assert!(p <= 4.0, "{p} まで行った");
        let st = step_at(&s, 9999.0);
        assert!(st <= 56.0, "{st} まで行った");
        // 目盛りが無いときに落ちない
        assert_eq!(bar_pos(&[], 5.0), 0.0);
        assert_eq!(step_at(&[0], 5.0), 0.0);
    }

    #[test]
    fn the_widths_go_from_tight_to_loose() {
        assert!(BAR_W.windows(2).all(|w| w[0] < w[1]), "幅が小さい順に並んでいない");
        assert!(BAR_W[0] >= 4.0, "細すぎて押せない");
    }

    #[test]
    fn the_track_header_has_room_for_what_is_in_it() {
        // 絵・名前・M・S・音量が1枚に収まる高さと幅があること。
        // ここを詰めると、どれかが重なって押せなくなる
        assert!(ROW_H >= ICON + 20.0, "行が絵より低い");
        assert!(LABEL_W >= ICON + 120.0, "音量のつまみを置く幅が無い");
    }

    #[test]
    fn the_shown_name_prefers_the_label() {
        let song = tonescript_song::load_str(
            r#"let BPM = 120;
               let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
               let VOICES = #{
                   lead: #{ ch: 0, patch: "piano", label: "主旋律" },
                   bass: #{ ch: 1, patch: "sub" },
               };"#,
        )
        .expect("読めるはず");
        assert_eq!(shown_name(&song, "lead"), "主旋律");
        // label が無ければパート名そのまま
        assert_eq!(shown_name(&song, "bass"), "bass");
        // 知らないパートでも落ちない
        assert_eq!(shown_name(&song, "なにこれ"), "なにこれ");
    }
}
