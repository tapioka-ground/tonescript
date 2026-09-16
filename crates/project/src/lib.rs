//! 編集の状態と、その保存。
//!
//! 曲そのもの（`.rhai`）は手で書くもので、こちらは触らない。
//! GUI で音符を動かしたり、音量の線を描いたりした結果だけを別に持つ。
//! 曲ファイルを書き換えないので、手で書いたコメントも式も消えない。
//!
//! 保存について
//! ------------
//! Python 版には自動保存が無かった。`edit.json` を直接上書きするだけで、
//! 落ちたらそこまでの作業が消える。DAW なら必ずある安全網なので、
//! ここでは最初から入れておく。
//!
//!   - 書くときは一度別名で書いてから差し替える（途中で落ちても壊れない）
//!   - 上書きする前のものを世代で残す（間違えても戻せる）
//!   - 直したら少し待って自動で保存する（押し忘れても消えない）
//!
//! なぜ JSON を自前で書くのか
//! --------------------------
//! 中身は数と文字列の入れ子だけで、serde を入れるほどのものではない。
//! 依存を減らすのがこの企画の目的の一つなので、ここは自分で書く。

pub mod history;
pub mod json;
pub mod store;

pub use history::{History, Tag};

use tonescript_song::model::{AudioTrack, Curve, Lane, MixCfg, Note};
use std::collections::HashMap;

/// 保存の形式。読むときに、知らない世代のものを弾くために持つ。
pub const FORMAT: u32 = 1;

/// 編集の状態。
///
/// 曲ファイルが作る譜面に「上書き」を重ねる形にしてある。
/// 触っていないパートは曲ファイルの生成に従うので、曲を直せばついてくる。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    /// どの曲に対する編集か
    pub song: String,
    /// パート -> 手で置いた音符。ここにあるパートは曲ファイルの生成を使わない
    pub notes: HashMap<String, Vec<Note>>,
    /// パート -> 線
    pub automation: HashMap<String, HashMap<Lane, Curve>>,
    /// パート -> 音量の倍率（線ではなく1つの値）
    pub gains: HashMap<String, f32>,
    /// パート -> 広がり・残響の送り・ダッキング。曲ファイルの `MIX` を上書きする
    pub mix: HashMap<String, MixCfg>,
    /// その場で録ったもの。曲ファイルの `AUDIO_TRACKS` に足す形で効く
    pub takes: HashMap<String, AudioTrack>,
    /// 黙らせているパート
    pub muted: Vec<String>,
    /// これだけ鳴らすパート。空なら全部鳴らす
    pub soloed: Vec<String>,
}

impl Project {
    pub fn new(song: &str) -> Self {
        Self { song: song.to_string(), ..Default::default() }
    }

    /// そのパートが鳴るか。ソロが1つでもあれば、それ以外は黙る。
    pub fn audible(&self, part: &str) -> bool {
        if !self.soloed.is_empty() {
            return self.soloed.iter().any(|p| p == part);
        }
        !self.muted.iter().any(|p| p == part)
    }

    /// 手で置いた音符があるパートか。
    pub fn is_edited(&self, part: &str) -> bool {
        self.notes.contains_key(part)
    }

    /// 音符を1つ置く。位置の順に入れる。
    pub fn add_note(&mut self, part: &str, note: Note) {
        let v = self.notes.entry(part.to_string()).or_default();
        let i = v.partition_point(|n| (n.pos, n.pitch) < (note.pos, note.pitch));
        v.insert(i, note);
    }

    /// その位置と音程にある音符を消す。消したものを返す。
    pub fn remove_note(&mut self, part: &str, pos: u32, pitch: i32) -> Option<Note> {
        let v = self.notes.get_mut(part)?;
        let i = v.iter().position(|n| n.pos <= pos && pos < n.pos + n.len.max(1) && n.pitch == pitch)?;
        Some(v.remove(i))
    }

    /// 曲ファイルが作った譜面へ、手で置いたぶんを重ねる。
    ///
    /// パート単位で差し替える。音符単位で混ぜると「曲ファイルを直したら
    /// 手で消したはずの音が戻る」が起きて、何が正なのか分からなくなる。
    pub fn overlay(&self, score: &mut HashMap<String, Vec<Note>>) {
        for (part, notes) in &self.notes {
            score.insert(part.clone(), notes.clone());
        }
        score.retain(|part, _| self.audible(part));
    }

    /// 何も編集していないか。保存するかどうかの判断に使う。
    pub fn is_pristine(&self) -> bool {
        self.notes.is_empty()
            && self.automation.is_empty()
            && self.gains.is_empty()
            && self.mix.is_empty()
            && self.takes.is_empty()
            && self.muted.is_empty()
            && self.soloed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(pos: u32, pitch: i32) -> Note {
        Note { pos, len: 4, pitch, vel: 100, mora: String::new() }
    }

    #[test]
    fn notes_stay_sorted() {
        let mut p = Project::new("x");
        p.add_note("lead", note(16, 60));
        p.add_note("lead", note(0, 64));
        p.add_note("lead", note(8, 62));
        let got: Vec<u32> = p.notes["lead"].iter().map(|n| n.pos).collect();
        assert_eq!(got, vec![0, 8, 16]);
    }

    #[test]
    fn remove_finds_the_note_under_the_click() {
        let mut p = Project::new("x");
        p.add_note("lead", note(8, 60)); // 8〜12 を占める
        // 音符の途中を指しても消える
        assert!(p.remove_note("lead", 10, 60).is_some());
        assert!(p.notes["lead"].is_empty());
        // 無い所は消えない
        assert!(p.remove_note("lead", 10, 60).is_none());
        assert!(p.remove_note("なにこれ", 0, 60).is_none());
    }

    #[test]
    fn mute_and_solo() {
        let mut p = Project::new("x");
        assert!(p.audible("lead"));
        p.muted.push("lead".into());
        assert!(!p.audible("lead"));
        assert!(p.audible("bass"));
        // ソロがあれば、ミュートより強い
        p.soloed.push("lead".into());
        assert!(p.audible("lead"));
        assert!(!p.audible("bass"));
    }

    #[test]
    fn overlay_replaces_whole_parts() {
        let mut score: HashMap<String, Vec<Note>> = HashMap::new();
        score.insert("lead".into(), vec![note(0, 60), note(4, 62)]);
        score.insert("bass".into(), vec![note(0, 40)]);

        let mut p = Project::new("x");
        p.add_note("lead", note(8, 72));
        p.overlay(&mut score);

        assert_eq!(score["lead"].len(), 1, "パートまるごと差し替わる");
        assert_eq!(score["lead"][0].pitch, 72);
        assert_eq!(score["bass"].len(), 1, "触っていないパートはそのまま");
    }

    #[test]
    fn overlay_drops_muted_parts() {
        let mut score: HashMap<String, Vec<Note>> = HashMap::new();
        score.insert("lead".into(), vec![note(0, 60)]);
        score.insert("bass".into(), vec![note(0, 40)]);
        let mut p = Project::new("x");
        p.muted.push("bass".into());
        p.overlay(&mut score);
        assert!(score.contains_key("lead"));
        assert!(!score.contains_key("bass"), "黙らせたパートが残っている");
    }

    #[test]
    fn pristine_until_touched() {
        let mut p = Project::new("x");
        assert!(p.is_pristine());
        p.add_note("lead", note(0, 60));
        assert!(!p.is_pristine());
    }
}
