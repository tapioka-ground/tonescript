//! パートを足す。
//!
//! DAW でいう「トラックを追加」。今まで曲ファイルを手で直すしかなかった所。
//!
//! どこへ書くか
//! ------------
//! **曲ファイル（`VOICES`）へ書く。** 音符やミキサーの値とは違って、
//! パートがあること自体は曲の骨組みで、編集の上書きで持つものではない。
//! 書き出しにも MIDI にも出るし、他の人が開いても同じものが見える。
//!
//! 鳴らすかどうかは別
//! ------------------
//! `VOICES` に足しただけでは、どの小節でも鳴らない（`ARRANGE` に居ない）。
//! アレンジビューで塗るか、ここで「全部の小節で鳴らす」を選ぶ。

use tonescript_song::Song;

/// 足そうとしているパート。窓が開いている間の下書き。
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    /// パートの名前。曲ファイルの鍵になるので英数字だけ
    pub name: String,
    /// 画面に出す名前
    pub label: String,
    /// 音色
    pub patch: String,
    /// 全部の小節で鳴らすか
    pub everywhere: bool,
}

impl Default for Spec {
    fn default() -> Self {
        Self {
            name: String::new(),
            label: String::new(),
            patch: "piano".into(),
            everywhere: true,
        }
    }
}

/// 使える音色の名前。曲が自分で作ったものも混ぜる。
pub fn patch_names(song: &Song) -> Vec<String> {
    let mut out: Vec<String> = tonescript_dsp::patch::NAMES
        .iter()
        .chain(tonescript_dsp::kit::NAMES.iter())
        .map(|s| s.to_string())
        .collect();
    // 曲が `PATCHES` で作ったものが勝つので、先に出す
    let mut own: Vec<String> = song.patches.keys().cloned().collect();
    own.sort();
    out.sort();
    out.dedup();
    own.extend(out);
    own.dedup();
    own
}

/// 使っていない MIDI チャンネルを1つ。
///
/// **9 は避ける。** あそこは打楽器と決まっていて、旋律を置くと
/// 書き出した MIDI が全部ドラムの音で鳴る
pub fn free_channel(song: &Song) -> u32 {
    let used: Vec<u8> = song.voices.values().map(|v| v.ch).collect();
    (0..16u32).find(|c| *c != 9 && !used.contains(&(*c as u8))).unwrap_or(15)
}

/// 足せる名前か。だめなら理由を返す。
pub fn check(spec: &Spec, song: &Song) -> Result<(), String> {
    let name = spec.name.trim();
    if name.is_empty() {
        return Err("名前がありません".into());
    }
    if name.len() > 24 {
        return Err("名前が長すぎます（24文字まで）".into());
    }
    // 曲ファイルの鍵としてそのまま書くので、素直な字だけにする
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("名前は英数字と _ だけにしてください（例: guitar2）".into());
    }
    if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Err("名前を数字で始めることはできません".into());
    }
    if song.voices.contains_key(name) {
        return Err(format!("{name} は既にあります"));
    }
    // 打楽器の名前とぶつかると、どちらが鳴るのか分からなくなる
    for kit in song.drum_kits.values() {
        if kit.contains_key(name) {
            return Err(format!("{name} はドラムキットの中にあります"));
        }
    }
    if spec.patch.trim().is_empty() {
        return Err("音色を選んでください".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Song {
        tonescript_song::load_str(
            r#"let BPM = 120;
               let SECTIONS = [["A", 1, "p", "k", "m", 1.0]];
               let VOICES = #{
                   lead:  #{ ch: 0, patch: "piano" },
                   drums: #{ ch: 9 },
               };
               let DRUM_KITS = #{ k: #{ kick: [36, [[0, 100]]] } };"#,
        )
        .expect("読めるはず")
    }

    #[test]
    fn a_plain_name_is_fine() {
        let s = Spec { name: "guitar2".into(), ..Default::default() };
        assert!(check(&s, &song()).is_ok());
    }

    #[test]
    fn names_that_would_break_the_file_are_refused() {
        for bad in ["", "ギター", "my guitar", "gui\"tar", "2nd", "gui-tar"] {
            let s = Spec { name: bad.into(), ..Default::default() };
            assert!(check(&s, &song()).is_err(), "{bad} が通った");
        }
    }

    #[test]
    fn a_name_already_in_use_is_refused() {
        let s = Spec { name: "lead".into(), ..Default::default() };
        assert!(check(&s, &song()).is_err(), "同じ名前が通った");
        // ドラムキットの中の名前も
        let s = Spec { name: "kick".into(), ..Default::default() };
        assert!(check(&s, &song()).is_err(), "打楽器と同じ名前が通った");
    }

    #[test]
    fn the_channel_skips_the_drum_lane_and_the_ones_in_use() {
        // 0 は lead、9 はドラム。次に空いているのは 1
        assert_eq!(free_channel(&song()), 1);
    }

    #[test]
    fn the_patch_list_has_everything_and_no_duplicates() {
        let names = patch_names(&song());
        assert!(names.len() > 100, "音色が {} しかない", names.len());
        assert!(names.iter().any(|n| n == "piano"), "組み込みが入っていない");
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "同じ名前が2度出ている");
    }
}
