//! ミキサー。パートごとの音量・左右・送りと、針。
//!
//! 触ると**鳴らしたまま変わる**。止めて作り直す必要はない。
//!
//! どこへ書くか
//! ------------
//! 触った値は曲ファイルではなく編集の状態（[`Project`]）へ入れる。
//! 曲ファイルの `GAINS` と `MIX` を上書きする形で、書き出しのときも同じ
//! 重ね方をする。**聞いたバランスがそのまま書き出される。**
//!
//! 曲ファイル側の値は触らないので、`GAINS` を直せば、触っていないパートは
//! ちゃんとついてくる。
//!
//! 針のこと
//! --------
//! 針は「前に読んでからの一番大きかったところ」を出す。画面は 60 分の1秒に
//! 1回しか見に来ないので、その瞬間の値を出すと山をほとんど取りこぼす。
//!
//! 落ちるのはゆっくりにしてある。上がりは即、下がりは緩やか。
//! 実際の針もそうなっていて、そうしないと目で追えない。

use egui::{Color32, Rect, Sense, Ui, Vec2};
use tonescript_engine::Engine;
use tonescript_project::Project;
use tonescript_song::model::MixCfg;
use tonescript_song::Song;

use crate::theme;

/// 針の落ちる速さ（1秒あたり何倍になるか）。
const FALL: f32 = 0.08;

/// 針の覚え書き。画面側にだけある（音側は最大値を置くだけ）。
#[derive(Default)]
pub struct Needles {
    /// パート名 -> 今の高さ
    level: std::collections::HashMap<String, f32>,
    /// パート名 -> 山を覚えておく高さ
    hold: std::collections::HashMap<String, (f32, f32)>,
    master: (f32, f32),
    master_hold: (f32, f32),
}

/// 針を落とす。上がりは即、下がりは緩やか。録音の入力にも使う。
pub fn fall(now: f32, was: f32, dt: f32) -> f32 {
    Needles::step(now, was, dt)
}

impl Needles {
    /// 針を進める。`dt` は前のフレームからの秒数。
    fn step(now: f32, was: f32, dt: f32) -> f32 {
        if now >= was {
            now // 上がりは即
        } else {
            (was * FALL.powf(dt)).max(now)
        }
    }

    fn hold_step(now: f32, was: (f32, f32), dt: f32) -> (f32, f32) {
        if now >= was.0 {
            (now, 0.0)
        } else if was.1 > 1.2 {
            // 1.2 秒見せたら落とし始める
            (Self::step(now, was.0, dt), was.1)
        } else {
            (was.0, was.1 + dt)
        }
    }

    pub fn tick(&mut self, engine: &Engine, parts: &[String], dt: f32) {
        for p in parts {
            let Some(i) = engine.plan().part_of(p) else { continue };
            let now = engine.meters().take_part(i);
            let was = self.level.get(p).copied().unwrap_or(0.0);
            let v = Self::step(now, was, dt);
            self.level.insert(p.clone(), v);
            let h = self.hold.get(p).copied().unwrap_or((0.0, 0.0));
            self.hold.insert(p.clone(), Self::hold_step(v, h, dt));
        }
        let (l, r) = engine.meters().take_master();
        self.master = (Self::step(l, self.master.0, dt), Self::step(r, self.master.1, dt));
        self.master_hold = (
            Self::hold_step(self.master.0, (self.master_hold.0, 0.0), dt).0.max(self.master.0),
            Self::hold_step(self.master.1, (self.master_hold.1, 0.0), dt).0.max(self.master.1),
        );
    }

    fn of(&self, part: &str) -> (f32, f32) {
        (
            self.level.get(part).copied().unwrap_or(0.0),
            self.hold.get(part).copied().unwrap_or((0.0, 0.0)).0,
        )
    }
}

/// 触ったこと。
///
/// **ここでは直接書き換えない。**「こう変えたい」を返して、画面側が
/// 取り消せる形で当てる。ここで書き換えると、取り消しのために毎フレーム
/// 編集の状態を丸ごと写すことになる。
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// 音量を変えた
    Gain { part: String, gain: f32 },
    /// 広がり・送り・凹み・音の整えを変えた
    Mix { part: String, mix: MixCfg },
    /// 黙らせる／戻す
    Mute(String),
    /// これだけ鳴らす／戻す
    Solo(String),
}

/// 目盛りは dB で切る。大きさそのままで描くと、下のほうが潰れて読めない。
fn db_of(v: f32) -> f32 {
    if v <= 1e-5 {
        return -60.0;
    }
    20.0 * v.log10()
}

/// -60〜+6 dB を 0〜1 へ。
pub fn bar_of(v: f32) -> f32 {
    ((db_of(v) + 60.0) / 66.0).clamp(0.0, 1.0)
}

/// 縦の針を1本描く。
fn meter(ui: &mut Ui, size: Vec2, level: f32, hold: f32) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, theme::PANEL);
    let h = rect.height() * bar_of(level);
    if h > 0.5 {
        let top = rect.bottom() - h;
        // 0dB を超えたところは赤。目で分かるように色を変える
        let color = if level > 1.0 {
            theme::RED
        } else if level > 0.7 {
            theme::ORANGE
        } else {
            theme::GREEN
        };
        p.rect_filled(Rect::from_min_max(egui::pos2(rect.left(), top), rect.max), 2.0, color);
    }
    if hold > 1e-4 {
        let y = rect.bottom() - rect.height() * bar_of(hold);
        let c = if hold > 1.0 { theme::RED } else { Color32::from_gray(190) };
        p.line_segment([egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)], (1.5, c));
    }
    // 0dB の線
    let y = rect.bottom() - rect.height() * bar_of(1.0);
    p.line_segment(
        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
        (1.0, Color32::from_gray(90)),
    );
}

/// ミキサーを描く。触られたことを返す。
pub fn panel(
    ui: &mut Ui,
    song: &Song,
    project: &Project,
    engine: &Engine,
    needles: &Needles,
    editing: &mut String,
) -> Vec<Edit> {
    let mut out = Vec::new();
    let parts: Vec<String> = song.edit_parts.clone();

    egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.horizontal(|ui| {
            for part in &parts {
                strip(ui, song, project, needles, editing, part, &mut out);
                ui.separator();
            }
            master_strip(ui, song, engine, needles);
        });
    });
    out
}

/// パート1本ぶん。
fn strip(
    ui: &mut Ui,
    song: &Song,
    project: &Project,
    needles: &Needles,
    editing: &mut String,
    part: &str,
    out: &mut Vec<Edit>,
) {
    // 今の値。触っていなければ曲ファイルの値
    let mut gain = project
        .gains
        .get(part)
        .copied()
        .unwrap_or_else(|| song.gains.get(part).copied().unwrap_or(1.0));
    let mut mix = project
        .mix
        .get(part)
        .copied()
        .unwrap_or_else(|| song.mix.get(part).copied().unwrap_or_default());

    ui.vertical(|ui| {
        ui.set_width(84.0);
        // 名前。押すと編集するパートが変わる
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
            ui.painter().rect_filled(rect, 2.0, theme::part_color(part));
            if ui.selectable_label(editing == part, part).clicked() {
                *editing = part.to_string();
            }
        });

        ui.horizontal(|ui| {
            // 針とフェーダーを並べる
            let (l, hold) = needles.of(part);
            meter(ui, Vec2::new(10.0, 120.0), l, hold);
            let r = ui.add(
                egui::Slider::new(&mut gain, 0.0..=4.0)
                    .vertical()
                    .show_value(false)
                    .step_by(0.01),
            );
            if r.changed() {
                out.push(Edit::Gain { part: part.to_string(), gain });
            }
            ui.vertical(|ui| {
                ui.add_space(46.0);
                ui.label(theme::dim(&format!("{:.2}", gain)));
                let db = db_of(gain);
                ui.label(theme::dim(&if gain <= 0.0 {
                    "—".to_string()
                } else {
                    format!("{db:+.1}dB")
                }));
            });
        });

        // 黙らせる・これだけ鳴らす
        ui.horizontal(|ui| {
            let muted = project.muted.iter().any(|m| m == part);
            let soloed = project.soloed.iter().any(|m| m == part);
            if ui.selectable_label(muted, "M").on_hover_text("黙らせる").clicked() {
                out.push(Edit::Mute(part.to_string()));
            }
            if ui.selectable_label(soloed, "S").on_hover_text("これだけ鳴らす").clicked() {
                out.push(Edit::Solo(part.to_string()));
            }
        });

        // 広がり・送り・ダッキング
        let mut touched = false;
        // 左右。-1〜1 なので、他のつまみと別に書く
        ui.horizontal(|ui| {
            ui.label(theme::dim("左右"));
            let r = ui.add(
                egui::DragValue::new(&mut mix.pan)
                    .speed(0.01)
                    .range(-1.0..=1.0)
                    .fixed_decimals(2)
                    .custom_formatter(|v, _| {
                        if v.abs() < 0.005 {
                            "中央".into()
                        } else if v < 0.0 {
                            format!("L{:.0}", -v * 100.0)
                        } else {
                            format!("R{:.0}", v * 100.0)
                        }
                    }),
            );
            if r.on_hover_text("-1 が左、0 が中央、+1 が右。線を書いてあればそちらが勝つ").changed() {
                touched = true;
            }
        });
        let mut knob = |ui: &mut Ui, label: &str, v: &mut f32, hi: f32, tip: &str| {
            ui.horizontal(|ui| {
                ui.label(theme::dim(label));
                let r = ui.add(
                    egui::DragValue::new(v).speed(0.01).range(0.0..=hi).fixed_decimals(2),
                );
                if r.on_hover_text(tip).changed() {
                    touched = true;
                }
            });
        };
        knob(ui, "幅", &mut mix.width, 4.0, "左右の広がり。0 で完全中央。低音は 0 のまま");
        knob(ui, "残", &mut mix.reverb, 2.0, "残響へ送る量。0 で送らない");
        knob(ui, "凹", &mut mix.duck, 2.0, "キックのたびに凹む量（サイドチェイン）");
        // 音の整え。dB なので別の見た目にする
        ui.add_space(2.0);
        let mut db = |ui: &mut Ui, label: &str, v: &mut f32, tip: &str| {
            ui.horizontal(|ui| {
                ui.label(theme::dim(label));
                let r = ui.add(
                    egui::DragValue::new(v)
                        .speed(0.1)
                        .range(-24.0..=24.0)
                        .fixed_decimals(1)
                        .suffix("dB"),
                );
                if r.on_hover_text(tip).changed() {
                    touched = true;
                }
            });
        };
        db(ui, "低", &mut mix.eq.low, "200Hz から下。ベースとキックがぶつかるときは、片方を削る");
        db(ui, "中", &mut mix.eq.mid, "1kHz のあたり。削ると引っ込み、上げると前に出る");
        db(ui, "高", &mut mix.eq.high, "4kHz から上。上げると明るく、削ると丸くなる");
        if touched {
            out.push(Edit::Mix { part: part.to_string(), mix });
        }
    });
}

/// 全体。
fn master_strip(ui: &mut Ui, song: &Song, engine: &Engine, needles: &Needles) {
    ui.vertical(|ui| {
        ui.set_width(120.0);
        ui.label(theme::head("全体"));
        ui.horizontal(|ui| {
            meter(ui, Vec2::new(10.0, 120.0), needles.master.0, needles.master_hold.0);
            meter(ui, Vec2::new(10.0, 120.0), needles.master.1, needles.master_hold.1);
            ui.vertical(|ui| {
                ui.add_space(30.0);
                let lufs = engine.meters().lufs();
                // 音圧。曲ファイルの狙い（MASTER_LUFS）と並べて出す
                ui.label(theme::dim(&if lufs <= -69.0 {
                    "— LUFS".to_string()
                } else {
                    format!("{lufs:.1} LUFS")
                }));
                ui.label(theme::dim(&format!("狙い {:.1}", song.master_lufs)));
                let peak = needles.master.0.max(needles.master.1);
                ui.label(theme::dim(&if peak <= 1e-5 {
                    "—".to_string()
                } else {
                    format!("ピーク {:+.1}dB", db_of(peak))
                }));
                if peak > 0.999 {
                    ui.colored_label(theme::RED, "天井に当たっている");
                }
            });
        });
        ui.label(theme::dim(&format!("書き出しの倍率 x{:.2}", engine.makeup())))
            .on_hover_text(
                "曲を読んだときに測った、書き出しと同じ音圧で聞くための倍率。\n\
                 これが効いているので、鳴らしている音と書き出した音の大きさが揃う",
            );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_is_in_decibels() {
        assert!((db_of(1.0) - 0.0).abs() < 1e-4);
        assert!((db_of(0.5) + 6.02).abs() < 0.01);
        assert_eq!(db_of(0.0), -60.0, "無音が -inf になっている");
        // 0dB は棒の上のほうに来る（下半分に潰れない）
        assert!(bar_of(1.0) > 0.85, "0dB が {} の高さ", bar_of(1.0));
        assert!(bar_of(0.5) > 0.7);
        assert_eq!(bar_of(0.0), 0.0);
        // 天井を超えても振り切れたままで、はみ出さない
        assert_eq!(bar_of(10.0), 1.0);
    }

    #[test]
    fn the_needle_rises_at_once_and_falls_slowly() {
        // 上がりは即。取りこぼすと山が見えない
        assert_eq!(Needles::step(0.8, 0.2, 0.016), 0.8);
        // 下がりは緩やか。1フレームでは少ししか下がらない
        let after = Needles::step(0.0, 1.0, 0.016);
        assert!(after > 0.9, "1フレームで {after} まで落ちた");
        // 1秒経てばほぼ落ちる
        let after = Needles::step(0.0, 1.0, 1.0);
        assert!(after < 0.1, "1秒経っても {after}");
        // 下の値より下には行かない
        assert_eq!(Needles::step(0.5, 1.0, 100.0), 0.5);
    }

    #[test]
    fn the_peak_mark_waits_before_it_drops() {
        // 山に当たったら覚える
        let mut h = Needles::hold_step(0.9, (0.0, 0.0), 0.016);
        assert_eq!(h.0, 0.9);
        // しばらくはそのまま
        for _ in 0..30 {
            h = Needles::hold_step(0.1, h, 0.016);
        }
        assert_eq!(h.0, 0.9, "すぐ落ちた");
        // 1.2 秒を過ぎたら落ち始める
        for _ in 0..60 {
            h = Needles::hold_step(0.1, h, 0.016);
        }
        assert!(h.0 < 0.9, "いつまでも残っている");
    }
}
