//! 見た目の決まりごと。
//!
//! Python 版の `ui.py` から色だけ持ってきた。手描きの楽器アイコンや
//! 部品は移していない（殻のままにしてある）。
//!
//! 色は役割の名前で呼ぶ。Apple の暗い画面用の値をそのまま使っている。

use egui::{Color32, FontId, RichText};

// -- 地の色。奥から手前へ、だんだん明るくなる
pub const BASE: Color32 = Color32::from_rgb(0x0c, 0x0c, 0x0e);
pub const SURFACE: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1e);
pub const RAISED: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2e);
pub const HOVER: Color32 = Color32::from_rgb(0x3a, 0x3a, 0x3c);

// -- 線
pub const LINE: Color32 = Color32::from_rgb(0x26, 0x26, 0x28);
pub const LINE_STRONG: Color32 = Color32::from_rgb(0x44, 0x44, 0x48);

// -- 文字
pub const LABEL: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const DIM: Color32 = Color32::from_rgb(0x98, 0x98, 0x9f);

// -- 差し色
pub const BLUE: Color32 = Color32::from_rgb(0x0a, 0x84, 0xff);
pub const RED: Color32 = Color32::from_rgb(0xff, 0x45, 0x3a);
pub const ORANGE: Color32 = Color32::from_rgb(0xff, 0x9f, 0x0a);
/// 針が振れているところ
pub const GREEN: Color32 = Color32::from_rgb(0x30, 0xd1, 0x58);
/// 針の下地
pub const PANEL: Color32 = Color32::from_rgb(0x18, 0x18, 0x1a);

// -- 再生ヘッド。鳴っているあいだは白くして目立たせる
pub const HEAD_ON: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
pub const HEAD_OFF: Color32 = Color32::from_rgb(0x8e, 0x8e, 0x93);

/// パートの色。曲ファイルの VOICES にも色はあるが、譜面はここで決める。
/// 同じパートがいつも同じ色なら、曲を切り替えても目が迷わない。
pub fn part_color(part: &str) -> Color32 {
    match part {
        "lead" => Color32::from_rgb(0xff, 0x9f, 0x43),
        "chords" => Color32::from_rgb(0x9c, 0x88, 0xff),
        "bass" => Color32::from_rgb(0x26, 0xde, 0x81),
        "sub" => Color32::from_rgb(0x00, 0xb8, 0x94),
        "arp" => Color32::from_rgb(0x54, 0xa0, 0xff),
        "drums" => Color32::from_rgb(0xfd, 0x79, 0xa8),
        "perc" => Color32::from_rgb(0x64, 0xd2, 0xff),
        "vocal" => Color32::from_rgb(0xff, 0x4d, 0x6d),
        "fx" => Color32::from_rgb(0xb2, 0xbe, 0xc3),
        _ => Color32::from_rgb(0x8e, 0x8e, 0x93),
    }
}

pub fn head(t: &str) -> RichText {
    RichText::new(t).size(11.0).strong().color(DIM)
}

pub fn dim(t: &str) -> RichText {
    RichText::new(t).size(12.0).color(DIM)
}

pub fn install(ctx: &egui::Context) {
    let mut s = (*ctx.style()).clone();
    let v = &mut s.visuals;
    v.dark_mode = true;
    v.panel_fill = SURFACE;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BASE;
    v.override_text_color = Some(LABEL);
    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.inactive.bg_fill = RAISED;
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.active.bg_fill = BLUE;
    v.selection.bg_fill = BLUE.linear_multiply(0.45);
    s.spacing.item_spacing = egui::vec2(6.0, 4.0);
    s.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(16.0));
    s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(12.5));
    s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(12.5));
    s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(11.5));
    ctx.set_style(s);
}
