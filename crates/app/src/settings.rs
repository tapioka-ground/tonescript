//! 曲の設定。DAW でいうプロジェクト設定。
//!
//! 直したものは曲ファイル（`.rhai`）へ書き戻す。画面と曲ファイルで
//! 値が食い違うのが一番よくないので、曲ファイルを正本にする。
//!
//! 書き戻すのは行頭の決まった値と `SECTIONS` だけ。旋律や和音のような
//! 作りものは触らない。あれは手で書くもの。

use tonescript_song::model::{Meter, Section};
use tonescript_song::rewrite;
use std::path::Path;

/// 画面で編集している値。窓を開いたときに曲から写して、
/// 「保存」で曲ファイルへ返す。
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub title: String,
    pub bpm: f32,
    pub key: String,
    pub master_lufs: f32,
    pub master_gain: f32,
    pub sections: Vec<Section>,
}

impl Draft {
    pub fn from_song(s: &tonescript_song::Song) -> Self {
        Self {
            title: s.title.clone(),
            bpm: s.bpm,
            key: s.key.clone(),
            master_lufs: s.master_lufs,
            master_gain: s.master_gain,
            sections: s.sections.clone(),
        }
    }

    /// 何か変わっているか。変わっていなければ書かない。
    pub fn differs_from(&self, s: &tonescript_song::Song) -> bool {
        *self != Self::from_song(s)
    }

    /// 全部で何小節か。
    pub fn bars(&self) -> u32 {
        self.sections.iter().map(|x| x.bars).sum()
    }

    /// だいたい何秒の曲になるか。
    ///
    /// テンポが途中で変わる曲では合わないが、目安として出す。
    pub fn rough_seconds(&self) -> f32 {
        if self.bpm <= 0.0 {
            return 0.0;
        }
        let beats: f32 = self.sections.iter().map(|s| (s.bars * s.meter.num) as f32).sum();
        beats * 60.0 / self.bpm
    }

    /// 書ける値か確かめる。だめなら理由を返す。
    pub fn check(&self) -> Result<(), String> {
        if !(20.0..=400.0).contains(&self.bpm) {
            return Err(format!("BPM が {} です。20〜400 の間に", self.bpm));
        }
        if self.sections.is_empty() {
            return Err("セクションが1つもありません".into());
        }
        if self.sections.len() > 64 {
            return Err("セクションが多すぎます（64まで）".into());
        }
        for (i, s) in self.sections.iter().enumerate() {
            if s.name.trim().is_empty() {
                return Err(format!("{}番目のセクションに名前がありません", i + 1));
            }
            if s.name.contains('"') || s.name.contains('\\') {
                return Err(format!("セクション名に使えない文字があります: {}", s.name));
            }
            if !(1..=512).contains(&s.bars) {
                return Err(format!("{} の小節数が {} です。1〜512 の間に", s.name, s.bars));
            }
            if !(1..=32).contains(&s.meter.num) || ![1, 2, 4, 8, 16].contains(&s.meter.den) {
                return Err(format!("{} の拍子 {} は扱えません", s.name, s.meter));
            }
            if !(0.0..=4.0).contains(&s.gain) {
                return Err(format!("{} の音量が {} です。0〜4 の間に", s.name, s.gain));
            }
        }
        if !(-40.0..=0.0).contains(&self.master_lufs) {
            return Err(format!("音圧の目標が {} です。-40〜0 の間に", self.master_lufs));
        }
        if !(0.0..=4.0).contains(&self.master_gain) {
            return Err(format!("全体音量が {} です。0〜4 の間に", self.master_gain));
        }
        Ok(())
    }

    /// 曲ファイルへ書き戻す。
    ///
    /// 書く前に読み直して確かめる。通らなければ何も書かない。
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.check()?;
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let edit = rewrite::Edit {
            title: Some(self.title.clone()),
            bpm: Some(self.bpm),
            key: Some(self.key.clone()),
            master_lufs: Some(self.master_lufs),
            master_gain: Some(self.master_gain),
            sections: Some(self.sections.clone()),
        };
        let out = edit.apply(&src).map_err(|e| e.to_string())?;
        write_atomic(path, &out)
    }
}

/// 別名で書き切ってから差し替える。途中で落ちても曲ファイルを壊さない。
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let tmp = path.with_extension("rhai.tmp");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// 新しいセクションの雛形。
pub fn new_section(name: &str, meter: Meter) -> Section {
    Section {
        name: name.into(),
        bars: 8,
        bass: "plain".into(),
        kit: "light".into(),
        arp: "main".into(),
        gain: 1.0,
        meter,
    }
}

/// 曲ファイルに出てくるベース型・キット・リフ型の名前。
/// 選ばせるために集める。書いていないものを選ぶと鳴らないので。
pub fn known_names(s: &tonescript_song::Song) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut bass: Vec<String> = s.bass_patterns.keys().cloned().collect();
    let mut kit: Vec<String> = s.drum_kits.keys().cloned().collect();
    let mut arp: Vec<String> = s.arp_patterns.keys().cloned().collect();
    bass.sort();
    kit.sort();
    arp.sort();
    (bass, kit, arp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> tonescript_song::Song {
        tonescript_song::load_str(
            r#"let TITLE = "t";
               let BPM = 128;
               let KEY = "A minor";
               let TEMPO_MAP = #{ "1": 128 };
               let SECTIONS = [
                   ["イントロ", 8, "plain", "light", "main", 0.85],
                   ["サビ", 8, "octa", "full", "high", 1.00],
               ];
               let BASS_PATTERNS = #{ plain: [[0,4,0]], octa: [[0,2,0]] };
               let ARP_PATTERNS = #{ main: [[0,2,0,0]], high: [[0,2,0,1]] };
               let DRUM_KITS = #{ light: #{ kick: [36, [[0,100]]] },
                                  full: #{ kick: [36, [[0,126]]] } };
               let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
               let MASTER_GAIN = 1.0;
               let MASTER_LUFS = -9;
            "#,
        )
        .unwrap()
    }

    fn tmp_song(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("mp_set_{name}_{}.rhai", std::process::id()));
        std::fs::write(
            &p,
            "// 説明。BPM の話も書いてある。\n\
             let TITLE = \"もと\";\n\
             let BPM = 128;\n\
             let KEY = \"A minor\";\n\
             let TEMPO_MAP = #{ \"1\": 128 };\n\
             let SECTIONS = [\n\
             \x20   [\"イントロ\", 8, \"plain\", \"light\", \"main\", 0.85],\n\
             ];\n\
             let VOICES = #{ lead: #{ ch: 0, patch: \"piano\" } };\n\
             let MASTER_GAIN = 1.0;\n\
             let MASTER_LUFS = -9;\n",
        )
        .unwrap();
        p
    }

    #[test]
    fn draft_reads_the_song() {
        let s = song();
        let d = Draft::from_song(&s);
        assert_eq!(d.title, "t");
        assert_eq!(d.bpm, 128.0);
        assert_eq!(d.bars(), 16);
        assert!(!d.differs_from(&s), "写しただけで違うと言われた");
    }

    #[test]
    fn rough_seconds_is_about_right() {
        let mut d = Draft::from_song(&song());
        // 16小節 x 4拍 = 64拍。128BPM なら 30 秒
        assert!((d.rough_seconds() - 30.0).abs() < 0.1, "{}", d.rough_seconds());
        // 3/4 にすると 16小節 x 3拍 = 48拍 = 22.5 秒
        for s in &mut d.sections {
            s.meter = Meter::new(3, 4);
        }
        assert!((d.rough_seconds() - 22.5).abs() < 0.1, "{}", d.rough_seconds());
    }

    #[test]
    fn bad_values_are_refused() {
        let base = Draft::from_song(&song());
        let cases: Vec<(Draft, &str)> = vec![
            (Draft { bpm: 5.0, ..base.clone() }, "BPM 低すぎ"),
            (Draft { bpm: 1000.0, ..base.clone() }, "BPM 高すぎ"),
            (Draft { sections: vec![], ..base.clone() }, "セクション無し"),
            (Draft { master_lufs: 5.0, ..base.clone() }, "音圧が正"),
            (Draft { master_gain: -1.0, ..base.clone() }, "音量が負"),
            (
                Draft {
                    sections: vec![new_section("", Meter::default())],
                    ..base.clone()
                },
                "名前が空",
            ),
            (
                Draft {
                    sections: vec![Section { bars: 0, ..new_section("A", Meter::default()) }],
                    ..base.clone()
                },
                "小節 0",
            ),
            (
                Draft {
                    sections: vec![Section {
                        meter: Meter { num: 4, den: 3 },
                        ..new_section("A", Meter::default())
                    }],
                    ..base.clone()
                },
                "拍子の分母が 3",
            ),
        ];
        for (d, why) in cases {
            assert!(d.check().is_err(), "通ってしまった: {why}");
        }
        assert!(base.check().is_ok(), "まともな値が弾かれた");
    }

    #[test]
    fn saving_writes_back_and_keeps_comments() {
        let p = tmp_song("save");
        let before = std::fs::read_to_string(&p).unwrap();
        let mut d = Draft::from_song(&tonescript_song::load_file(&p).unwrap());
        d.title = "新しい名前".into();
        d.bpm = 174.0;
        d.sections[0].bars = 4;
        d.sections.push(new_section("サビ", Meter::new(7, 8)));
        d.save(&p).expect("書けるはず");

        let after = std::fs::read_to_string(&p).unwrap();
        assert!(after.contains("// 説明。BPM の話も書いてある。"), "コメントが消えた");
        assert!(after.contains("let VOICES ="), "他の行が消えた");
        assert_ne!(before, after);

        let s = tonescript_song::load_file(&p).expect("開けるはず");
        assert_eq!(s.title, "新しい名前");
        assert_eq!(s.bpm, 174.0);
        assert_eq!(s.tempo_map[&1], 174.0, "テンポの節目が付いてきていない");
        assert_eq!(s.sections.len(), 2);
        assert_eq!(s.bars(), 4 + 8);
        assert_eq!(s.meter_at(5), Meter::new(7, 8));
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn a_bad_draft_does_not_touch_the_file() {
        let p = tmp_song("bad");
        let before = std::fs::read_to_string(&p).unwrap();
        let mut d = Draft::from_song(&tonescript_song::load_file(&p).unwrap());
        d.bpm = 9999.0;
        assert!(d.save(&p).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), before, "壊れた値で書いた");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn no_temp_file_is_left_behind() {
        let p = tmp_song("tmp");
        let d = Draft::from_song(&tonescript_song::load_file(&p).unwrap());
        d.save(&p).unwrap();
        assert!(!p.with_extension("rhai.tmp").exists(), "一時ファイルが残っている");
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn known_names_are_collected() {
        let (bass, kit, arp) = known_names(&song());
        assert_eq!(bass, vec!["octa", "plain"]);
        assert_eq!(kit, vec!["full", "light"]);
        assert_eq!(arp, vec!["high", "main"]);
    }
}
