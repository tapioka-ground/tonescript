//! 取り消しとやり直し。
//!
//! 何を覚えるか
//! ------------
//! 差分ではなく、編集の状態まるごとを覚える。差分にすると「その操作の
//! 逆は何か」を操作ごとに書くことになり、操作を1つ足すたびに取り消しが
//! 壊れる余地ができる。ここで持つのは音符と線だけで、曲ファイルも音声も
//! 入っていないので、まるごと覚えても小さい。
//!
//! まとめかた
//! ----------
//! 音符を1つドラッグすると、指を動かしているあいだ毎フレーム値が変わる。
//! そのたびに1段積むと、Ctrl+Z を 60 回押しても元に戻らない。
//! 同じ操作が続いているあいだは1段にまとめる（`Tag` が同じなら積まない）。
//!
//! 上限
//! ----
//! 段数には上限を置く。古いものから落とす。無制限にすると、長く作業した
//! ときに覚えているぶんだけメモリを食い続ける。

use crate::Project;

/// 何をしている最中か。同じものが続くあいだは1段にまとめる。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    /// まとめない。1回ごとに1段
    Once,
    /// 音符を動かしている（パートと何番目か）
    Move(String, usize),
    /// 長さを変えている
    Resize(String, usize),
    /// 線を描いている
    Curve(String, &'static str),
}

/// 覚えている段数の上限。
pub const DEPTH: usize = 200;

pub struct History {
    past: Vec<(Project, Tag)>,
    future: Vec<Project>,
    depth: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(DEPTH)
    }
}

impl History {
    pub fn new(depth: usize) -> Self {
        Self { past: Vec::new(), future: Vec::new(), depth: depth.max(1) }
    }

    /// これから変えるので、今の状態を覚える。**変える前**に呼ぶこと。
    ///
    /// 同じ `tag` が続いているあいだは積まない。ドラッグ中の1フレームごとに
    /// 積まないための仕組み。
    pub fn record(&mut self, current: &Project, tag: Tag) {
        if tag != Tag::Once {
            if let Some((_, last)) = self.past.last() {
                if *last == tag {
                    // 同じ操作の続き。すでに「始める前」を覚えている
                    self.future.clear();
                    return;
                }
            }
        }
        self.past.push((current.clone(), tag));
        if self.past.len() > self.depth {
            self.past.remove(0);
        }
        // 新しく何かしたら、やり直せる先は消える
        self.future.clear();
    }

    /// 操作の区切り。次の `record` は必ず新しい段になる。
    ///
    /// ドラッグを離したときに呼ぶ。呼ばないと、離してもう一度同じ音符を
    /// 掴んだときに前の段へまとめられてしまう。
    pub fn end_group(&mut self) {
        if let Some((_, tag)) = self.past.last_mut() {
            *tag = Tag::Once;
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// 1つ戻す。戻せたら true。
    pub fn undo(&mut self, current: &mut Project) -> bool {
        let Some((prev, _)) = self.past.pop() else { return false };
        let now = std::mem::replace(current, prev);
        self.future.push(now);
        true
    }

    /// 1つやり直す。
    pub fn redo(&mut self, current: &mut Project) -> bool {
        let Some(next) = self.future.pop() else { return false };
        let now = std::mem::replace(current, next);
        // やり直したものは、また戻せるようにしておく
        self.past.push((now, Tag::Once));
        if self.past.len() > self.depth {
            self.past.remove(0);
        }
        true
    }

    /// 全部忘れる。曲を切り替えたときなど。
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    /// 覚えている段数。画面に出すため。
    pub fn depth_used(&self) -> (usize, usize) {
        (self.past.len(), self.future.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonescript_song::model::Note;

    fn note(pos: u32, pitch: i32) -> Note {
        Note { pos, len: 4, pitch, vel: 100, mora: String::new() }
    }

    fn with(n: &[(u32, i32)]) -> Project {
        let mut p = Project::new("x");
        for (pos, pitch) in n {
            p.add_note("lead", note(*pos, *pitch));
        }
        p
    }

    #[test]
    fn undo_and_redo_walk_the_steps() {
        let mut h = History::default();
        let mut p = Project::new("x");

        h.record(&p, Tag::Once);
        p.add_note("lead", note(0, 60));
        h.record(&p, Tag::Once);
        p.add_note("lead", note(4, 62));

        assert_eq!(p.notes["lead"].len(), 2);
        assert!(h.can_undo());
        assert!(!h.can_redo());

        assert!(h.undo(&mut p));
        assert_eq!(p.notes["lead"].len(), 1, "1つ戻っていない");
        assert!(h.undo(&mut p));
        assert!(p.notes.is_empty() || p.notes["lead"].is_empty(), "最初に戻っていない");
        assert!(!h.undo(&mut p), "無いのに戻せた");

        assert!(h.can_redo());
        assert!(h.redo(&mut p));
        assert_eq!(p.notes["lead"].len(), 1);
        assert!(h.redo(&mut p));
        assert_eq!(p.notes["lead"].len(), 2, "やり直せていない");
        assert!(!h.redo(&mut p));
    }

    #[test]
    fn a_drag_is_one_step() {
        let mut h = History::default();
        let mut p = with(&[(0, 60)]);
        let before = p.clone();

        // 指を動かしているあいだ、毎フレーム record が呼ばれる
        for x in 1..=30u32 {
            h.record(&p, Tag::Move("lead".into(), 0));
            p.notes.get_mut("lead").unwrap()[0].pos = x;
        }
        h.end_group();

        assert_eq!(h.depth_used().0, 1, "ドラッグで段が増えている");
        assert!(h.undo(&mut p));
        assert_eq!(p, before, "1回で掴む前へ戻らない");
    }

    #[test]
    fn two_drags_are_two_steps() {
        let mut h = History::default();
        let mut p = with(&[(0, 60)]);

        for x in 1..=5u32 {
            h.record(&p, Tag::Move("lead".into(), 0));
            p.notes.get_mut("lead").unwrap()[0].pos = x;
        }
        h.end_group(); // 指を離した

        for x in 6..=10u32 {
            h.record(&p, Tag::Move("lead".into(), 0));
            p.notes.get_mut("lead").unwrap()[0].pos = x;
        }
        h.end_group();

        assert_eq!(h.depth_used().0, 2, "2回のドラッグが1段になっている");
        h.undo(&mut p);
        assert_eq!(p.notes["lead"][0].pos, 5, "2回目だけ戻るはず");
        h.undo(&mut p);
        assert_eq!(p.notes["lead"][0].pos, 0);
    }

    #[test]
    fn different_notes_do_not_merge() {
        let mut h = History::default();
        let mut p = with(&[(0, 60), (8, 64)]);
        h.record(&p, Tag::Move("lead".into(), 0));
        p.notes.get_mut("lead").unwrap()[0].pos = 1;
        h.record(&p, Tag::Move("lead".into(), 1));
        p.notes.get_mut("lead").unwrap()[1].pos = 9;
        assert_eq!(h.depth_used().0, 2, "別の音符が1段にまとまっている");
    }

    #[test]
    fn new_edit_clears_the_redo_side() {
        let mut h = History::default();
        let mut p = Project::new("x");
        h.record(&p, Tag::Once);
        p.add_note("lead", note(0, 60));
        h.undo(&mut p);
        assert!(h.can_redo());

        // 戻したあとに別のことをしたら、やり直せる先は消える
        h.record(&p, Tag::Once);
        p.add_note("lead", note(16, 70));
        assert!(!h.can_redo(), "枝分かれしたのに redo が残っている");
    }

    #[test]
    fn depth_is_capped() {
        let mut h = History::new(5);
        let mut p = Project::new("x");
        for i in 0..20u32 {
            h.record(&p, Tag::Once);
            p.add_note("lead", note(i * 4, 60));
        }
        assert_eq!(h.depth_used().0, 5, "上限を超えて覚えている");
        // 上限ぶんは戻せる
        for _ in 0..5 {
            assert!(h.undo(&mut p));
        }
        assert!(!h.undo(&mut p));
    }

    #[test]
    fn automation_and_mute_are_remembered_too() {
        use tonescript_song::model::{Curve, Lane};
        use std::collections::HashMap;
        let mut h = History::default();
        let mut p = Project::new("x");
        h.record(&p, Tag::Once);
        p.automation.insert(
            "lead".into(),
            HashMap::from([(Lane::Gain, Curve::new(vec![(0, 1.0), (32, 0.0)]))]),
        );
        p.muted.push("bass".into());

        h.undo(&mut p);
        assert!(p.automation.is_empty(), "線が戻っていない");
        assert!(p.muted.is_empty(), "ミュートが戻っていない");
    }

    #[test]
    fn clear_forgets_everything() {
        let mut h = History::default();
        let mut p = Project::new("x");
        h.record(&p, Tag::Once);
        p.add_note("lead", note(0, 60));
        h.undo(&mut p);
        h.clear();
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }
}
