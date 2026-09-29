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

use tonescript_song::model::{AudioTrack, BusCfg, Curve, Lane, MixCfg, Note};
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
    /// パート -> **触り始めた時点で曲ファイルが作っていた音符。**
    ///
    /// 手で直したぶんと曲ファイルの直しを混ぜるのに要る。
    /// 「元はこうだった」が分からないと、どちらが直したのか決められない
    /// （[`Project::resync`] を見よ）
    pub base: HashMap<String, Vec<Note>>,
    /// パート -> 線
    pub automation: HashMap<String, HashMap<Lane, Curve>>,
    /// パート -> 音量の倍率（線ではなく1つの値）
    pub gains: HashMap<String, f32>,
    /// パート -> 広がり・残響の送り・ダッキング。曲ファイルの `MIX` を上書きする
    pub mix: HashMap<String, MixCfg>,
    /// 小節 -> その小節で鳴らすパート。曲ファイルの `ARRANGE` を上書きする。
    ///
    /// **小節まるごと差し替える。** パート単位で足し引きを覚えると、
    /// 曲ファイルを直したときに、消したはずのパートが戻ってくる
    pub arrange: HashMap<u32, Vec<String>>,
    /// バス名 -> その設定。曲ファイルの `BUSES` を上書きする。
    /// **バスそのものを増やすのは曲ファイル側の仕事**（ここは設定だけ）
    pub buses: HashMap<String, BusCfg>,
    /// その場で録ったもの。曲ファイルの `AUDIO_TRACKS` に足す形で効く
    pub takes: HashMap<String, AudioTrack>,
    /// 黙らせているパート
    pub muted: Vec<String>,
    /// これだけ鳴らすパート。空なら全部鳴らす
    pub soloed: Vec<String>,
}

/// 混ぜた結果の内訳。何が起きたかを人に言うため。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sync {
    /// 曲ファイルの今の形をそのまま採った数
    pub from_song: usize,
    /// 人が足した・直したので残した数
    pub kept: usize,
    /// 人が消したので、曲ファイルにあっても入れなかった数
    pub dropped: usize,
    /// **人が直したのに残せなかった数。**
    ///
    /// 人が直した音を、曲ファイルが同じ所の別の音に差し替えたとき。
    /// どちらが正しいかは機械に決められない本物の衝突で、ここでは
    /// 曲ファイルを採る（音符が増えて濁るより、頼んだ通りに直るほうがいい）。
    /// **黙って消すのが一番よくない**ので、数だけでも持って帰る
    pub lost: usize,
}

/// 音符を見分ける鍵。同じ所の同じ高さなら同じ音符とみなす。
fn key(n: &Note) -> (u32, i32) {
    (n.pos, n.pitch)
}

/// 3方向マージ。[`Project::resync`] の中身。
fn merge(base: &[Note], user: &[Note], gen: &[Note]) -> (Vec<Note>, Sync) {
    use std::collections::HashSet;
    let base_keys: HashSet<(u32, i32)> = base.iter().map(key).collect();
    let user_keys: HashSet<(u32, i32)> = user.iter().map(key).collect();

    // 人が消したもの（元にはあって、今は無い）
    let dropped: HashSet<(u32, i32)> = base_keys.difference(&user_keys).copied().collect();
    // 人が足したもの（元に無くて、今はある）
    let added: Vec<Note> =
        user.iter().filter(|n| !base_keys.contains(&key(n))).cloned().collect();
    // 人が直したもの（両方にあって、中身が違う）
    let changed: Vec<Note> = user
        .iter()
        .filter(|n| {
            base.iter().any(|b| key(b) == key(n) && b != *n)
        })
        .cloned()
        .collect();

    let mut report = Sync::default();
    let mut out: Vec<Note> = Vec::with_capacity(gen.len() + added.len());
    // 人が直したぶんのうち、置き場が見つかったもの
    let mut placed: HashSet<(u32, i32)> = HashSet::new();
    for n in gen {
        if dropped.contains(&key(n)) {
            report.dropped += 1;
            continue;
        }
        match changed.iter().find(|c| key(c) == key(n)) {
            Some(c) => {
                out.push(c.clone());
                placed.insert(key(c));
                report.kept += 1;
            }
            None => {
                out.push(n.clone());
                report.from_song += 1;
            }
        }
    }
    // 人が足したぶん。**同じ所に曲ファイルの音があっても、人のほうを採る。**
    //
    // 飛ばすと「人が足したものは残る」の決まりが、長さと強さについてだけ
    // 破れる（音は出るが、置いたときの長さではなくなる）
    for n in added {
        match out.iter().position(|o| key(o) == key(&n)) {
            Some(i) => {
                out[i] = n;
                report.from_song -= 1;
                report.kept += 1;
            }
            None => {
                out.push(n);
                report.kept += 1;
            }
        }
    }
    // 人が直したのに、曲ファイルがその所を別の音へ差し替えてしまったぶん。
    // 本物の衝突。曲ファイルを採ったうえで、数だけ持って帰る
    report.lost = changed.iter().filter(|c| !placed.contains(&key(c))).count();
    out.sort_by_key(|n| (n.pos, n.pitch));
    (out, report)
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

    /// その小節でそのパートが鳴るか。上書きがあればそちらが勝つ。
    ///
    /// `song` 側の答えを渡してもらう形にしてある。曲を知らないここで
    /// 「上書きが無ければ false」にすると、触っていない小節が全部黙る
    pub fn plays(&self, bar: u32, part: &str, in_song: bool) -> bool {
        match self.arrange.get(&bar) {
            Some(v) => v.iter().any(|p| p == part),
            None => in_song,
        }
    }

    /// その小節でそのパートを鳴らすかどうかを決める。
    ///
    /// 上書きがまだ無ければ、まず曲ファイルの通りに写してから触る。
    /// いきなり1つだけ書くと、その小節の他のパートが全部消える
    pub fn set_plays(&mut self, bar: u32, part: &str, on: bool, now: &[String]) {
        let v = self.arrange.entry(bar).or_insert_with(|| now.to_vec());
        let at = v.iter().position(|p| p == part);
        match (on, at) {
            (true, None) => v.push(part.to_string()),
            (false, Some(i)) => {
                v.remove(i);
            }
            _ => {}
        }
    }

    /// 手で置いた音符があるパートか。
    pub fn is_edited(&self, part: &str) -> bool {
        self.notes.contains_key(part)
    }

    /// そのパートを手元へ写す。**触り始めるときに1回だけ呼ぶ。**
    ///
    /// 写した中身を `base` にも覚えておく。あとで曲ファイルが直されたとき、
    /// 「曲ファイルが変えた所」と「人が変えた所」を見分けるのに要る
    pub fn take_over(&mut self, part: &str, generated: &[Note]) {
        self.notes.entry(part.to_string()).or_insert_with(|| generated.to_vec());
        self.base.entry(part.to_string()).or_insert_with(|| generated.to_vec());
    }

    /// 手編集をやめて、曲ファイルの生成へ戻す。
    pub fn give_back(&mut self, part: &str) {
        self.notes.remove(part);
        self.base.remove(part);
    }

    /// 混ぜるものがあるか。無ければ何もしなくていい。
    ///
    /// 譜面を組み直すたびに呼ばれるので、先に安く判る道を用意しておく
    pub fn needs_resync(&self, generated: &HashMap<String, Vec<Note>>) -> bool {
        self.base.iter().any(|(part, base)| {
            generated.get(part).is_some_and(|gen| gen != base) && self.notes.contains_key(part)
        })
    }

    /// 曲ファイルが直されたぶんを、手編集へ**混ぜる**。
    ///
    /// これが無いと、音符を1つ触っただけでそのパートが曲ファイルから
    /// 切り離され、以後 `.rhai` をいくら直しても画面に出てこない。
    /// 「画面を開いたまま AI と直していく」がそこで止まる。
    ///
    /// やり方は版管理の3方向マージと同じ。
    ///
    /// - `base`（触り始めた時点の生成） … 元
    /// - `notes`（今の手編集） … 人が直したもの
    /// - `generated`（今の曲ファイルの生成） … 曲ファイルが直したもの
    ///
    /// 音符は `(位置, 音程)` で同じものとみなす。
    ///
    /// - 人が消したものは、曲ファイル側にあっても消えたまま
    /// - 人が足したものは残る（**同じ所に曲ファイルの音があっても人が勝つ**）
    /// - 人が長さや強さを変えたものは、人の値が勝つ
    /// - **人が触っていないものは、曲ファイルの今の形になる** ← これが要点
    ///
    /// 決められない場合が1つある。人が直した音を、曲ファイルが同じ所の
    /// **別の音へ差し替えた**とき。どちらが正しいかは機械に決められない。
    /// ここでは曲ファイルを採る（音符が増えて濁るより、頼んだ通りに直る
    /// ほうがいい）。そのぶんは `lost` に数えて人に見せる。
    ///
    /// 返すのは、混ぜたパートと [`Sync`]（内訳）。
    pub fn resync(&mut self, generated: &HashMap<String, Vec<Note>>) -> Vec<(String, Sync)> {
        let mut out = Vec::new();
        for (part, base) in self.base.clone() {
            let Some(gen) = generated.get(&part) else { continue };
            if *gen == base {
                continue; // 曲ファイル側は変わっていない
            }
            let Some(user) = self.notes.get(&part).cloned() else { continue };
            let (merged, report) = merge(&base, &user, gen);
            self.notes.insert(part.clone(), merged);
            self.base.insert(part.clone(), gen.clone());
            out.push((part, report));
        }
        out
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
            && self.arrange.is_empty()
            && self.buses.is_empty()
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
    fn an_untouched_bar_follows_the_song_file() {
        let p = Project::new("x");
        // 上書きが無いうちは、曲ファイルの答えをそのまま返すこと。
        // ここで false を返すと、触っていない小節まで全部黙る
        assert!(p.plays(1, "lead", true));
        assert!(!p.plays(1, "lead", false));
    }

    #[test]
    fn touching_one_part_keeps_the_others_in_that_bar() {
        let mut p = Project::new("x");
        let now = vec!["lead".to_string(), "bass".to_string()];
        // bass だけ黙らせる。lead は残ること
        p.set_plays(3, "bass", false, &now);
        assert!(p.plays(3, "lead", true), "触っていない lead まで消えた");
        assert!(!p.plays(3, "bass", true));
        // 足すほうも
        p.set_plays(3, "arp", true, &now);
        assert!(p.plays(3, "arp", false), "足したのに鳴らない");
        // 同じことを二度やっても増えない
        p.set_plays(3, "arp", true, &now);
        assert_eq!(p.arrange[&3].iter().filter(|x| *x == "arp").count(), 1);
    }

    #[test]
    fn a_bar_can_be_emptied_completely() {
        let mut p = Project::new("x");
        let now = vec!["lead".to_string()];
        p.set_plays(2, "lead", false, &now);
        assert!(p.arrange.contains_key(&2), "空にした覚えが残っていない");
        assert!(!p.plays(2, "lead", true), "曲ファイル側が勝ってしまった");
    }

    fn n(pos: u32, pitch: i32, len: u32) -> Note {
        Note { pos, len, pitch, vel: 100, mora: String::new() }
    }

    /// **曲ファイルの直しが、手で触ったパートにも届くこと。**
    ///
    /// ここが効かないと、音符を1つ触った瞬間にそのパートが曲ファイルから
    /// 切り離され、`.rhai` をいくら直しても画面に出てこない。
    /// 「画面を開いたまま AI と直していく」がそこで止まる
    #[test]
    fn the_song_file_still_reaches_a_part_you_have_edited() {
        let mut p = Project::new("x");
        // 曲ファイルが3音作っていた。そこを触り始める
        let gen = vec![n(0, 60, 4), n(4, 62, 4), n(8, 64, 4)];
        p.take_over("arp", &gen);
        // 人が1音足して、1音消して、1音の長さを変えた
        p.notes.insert(
            "arp".into(),
            vec![n(0, 60, 8), n(8, 64, 4), n(12, 67, 4)],
        );

        // 曲ファイル側が変わった（和音が変わって音程が上がった）
        let mut now = HashMap::new();
        now.insert("arp".to_string(), vec![n(0, 65, 4), n(4, 67, 4), n(8, 69, 4)]);
        let report = p.resync(&now);

        assert_eq!(report.len(), 1, "混ぜられていない");
        let got = &p.notes["arp"];
        let pitches: Vec<i32> = got.iter().map(|x| x.pitch).collect();
        // 曲ファイルの新しい音程が入っていること
        assert!(pitches.contains(&65), "曲ファイルの直しが届いていない: {pitches:?}");
        assert!(pitches.contains(&67));
        assert!(pitches.contains(&69));
        // 人が足した音は残ること
        assert!(got.iter().any(|x| x.pos == 12 && x.pitch == 67), "足した音が消えた");
        // 人が消した音（位置4・音程62）は、曲ファイルが作り直しても戻らないこと…
        // ただし今回は曲ファイル側の位置4が別の音程(67)になっているので、
        // 「消した音」とは別物として入る。ここは位置と音程で見ている結果
        assert_eq!(p.base["arp"], now["arp"], "元を覚え直していない");
    }

    #[test]
    fn what_the_person_changed_wins() {
        let mut p = Project::new("x");
        let gen = vec![n(0, 60, 4), n(4, 62, 4)];
        p.take_over("lead", &gen);
        // 長さだけ変えた
        p.notes.insert("lead".into(), vec![n(0, 60, 16), n(4, 62, 4)]);

        // 曲ファイル側も同じ音符の長さを変えてきた
        let mut now = HashMap::new();
        now.insert("lead".to_string(), vec![n(0, 60, 2), n(4, 62, 4), n(8, 64, 4)]);
        p.resync(&now);

        let got = &p.notes["lead"];
        let first = got.iter().find(|x| x.pos == 0).unwrap();
        assert_eq!(first.len, 16, "人が変えた長さが曲ファイルに負けた");
        // 曲ファイルが足した音符は入ること
        assert!(got.iter().any(|x| x.pos == 8), "曲ファイルが足した音が来ていない");
    }

    #[test]
    fn a_note_the_person_deleted_stays_deleted() {
        let mut p = Project::new("x");
        let gen = vec![n(0, 60, 4), n(4, 62, 4)];
        p.take_over("lead", &gen);
        p.notes.insert("lead".into(), vec![n(0, 60, 4)]); // 62 を消した

        // 曲ファイルは変わったが、消した音はそのまま作り続けている
        let mut now = HashMap::new();
        now.insert("lead".to_string(), vec![n(0, 60, 4), n(4, 62, 4), n(8, 64, 4)]);
        let report = p.resync(&now);

        let got = &p.notes["lead"];
        assert!(!got.iter().any(|x| x.pos == 4 && x.pitch == 62), "消した音が戻った");
        assert!(got.iter().any(|x| x.pos == 8), "曲ファイルが足した音が来ていない");
        assert_eq!(report[0].1.dropped, 1);
    }

    /// **人が置いた音の長さが、曲ファイルに上書きされないこと。**
    ///
    /// 「人が足したものは残る」と決めたのに、同じ所に曲ファイルも音を
    /// 作ってくると、長さだけ曲ファイルのものになっていた
    #[test]
    fn a_note_the_person_placed_keeps_its_length() {
        let mut p = Project::new("x");
        p.take_over("lead", &[]);
        p.notes.insert("lead".into(), vec![n(0, 69, 8)]); // 人が置いた。長さ8

        // 曲ファイルも同じ所に作ってきた。長さは4
        let mut now = HashMap::new();
        now.insert("lead".to_string(), vec![n(0, 69, 4)]);
        let report = p.resync(&now);

        let got = &p.notes["lead"];
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].len, 8, "人が置いた長さが曲ファイルに負けた");
        assert_eq!(report[0].1.kept, 1, "人のぶんとして数えていない");
        assert_eq!(report[0].1.from_song, 0);
    }

    /// **人が直した音を曲ファイルが別物に差し替えたら、黙って消さないこと。**
    ///
    /// これは本物の衝突で、どちらが正しいかは機械に決められない。
    /// 曲ファイルを採る（音符が増えて濁るより、頼んだ通りに直るほうがいい）
    /// が、**何が起きたかは必ず言う**
    #[test]
    fn a_real_conflict_is_reported_not_swallowed() {
        let mut p = Project::new("x");
        p.take_over("lead", &[n(0, 69, 4)]);
        p.notes.insert("lead".into(), vec![n(0, 69, 8)]); // 人は長さを直した

        // 曲ファイルは同じ所の音程を変えてきた
        let mut now = HashMap::new();
        now.insert("lead".to_string(), vec![n(0, 71, 4)]);
        let report = p.resync(&now);

        let got = &p.notes["lead"];
        assert_eq!(got.len(), 1, "音符が増えている: {got:?}");
        assert_eq!(got[0].pitch, 71, "曲ファイルの直しが入っていない");
        // **黙って消さない。** 数だけでも人に見せる
        assert_eq!(report[0].1.lost, 1, "消えたことが伝わらない");
    }

    #[test]
    fn nothing_happens_when_the_song_file_did_not_change() {
        let mut p = Project::new("x");
        let gen = vec![n(0, 60, 4)];
        p.take_over("lead", &gen);
        p.notes.insert("lead".into(), vec![n(0, 60, 4), n(4, 62, 4)]);
        let mut now = HashMap::new();
        now.insert("lead".to_string(), gen.clone());
        assert!(p.resync(&now).is_empty(), "変わっていないのに混ぜた");
        assert_eq!(p.notes["lead"].len(), 2, "手編集が触られた");
    }

    #[test]
    fn giving_a_part_back_forgets_the_snapshot_too() {
        let mut p = Project::new("x");
        p.take_over("lead", &[n(0, 60, 4)]);
        p.give_back("lead");
        assert!(!p.notes.contains_key("lead"));
        assert!(!p.base.contains_key("lead"), "元だけ残ると、次に触ったとき古い元で混ざる");
    }

    #[test]
    fn pristine_until_touched() {
        let mut p = Project::new("x");
        assert!(p.is_pristine());
        p.add_note("lead", note(0, 60));
        assert!(!p.is_pristine());
    }
}
