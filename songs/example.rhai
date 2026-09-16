// 曲の雛形。これを写して自分の曲を作る。
//
//     copy songs/example.rhai songs/mysong.rhai
//     tonescript render mysong
//
// 位置と長さの単位はすべて「16分音符いくつ分」。1小節 = 16。
// 各小節の旋律の合計は 16 ちょうどに揃えること（読み込み時に検算する）。
//
// 構成（全16小節）:
//    1- 8  イントロ  薄く始める
//    9-16  サビ      全部鳴らす

let TITLE = "Tonescript example";
let BPM = 128;
let KEY = "A minor";

// テンポの節目。通し小節 -> BPM。節目の間は補間される。
let TEMPO_MAP = #{ "1": 128 };
// 節目の繋ぎ方。"linear" は着地で傾きが一段落ちて角が出る。
// "smooth" は両端が滑らかで、気づかれずに前へ進む感じだけ残る。
let TEMPO_CURVE = "smooth";

// ------------------------------------------------------------------ 構成
// [名前, 小節数, ベース型, ドラムキット, リフ型, ベロシティ倍率]
let SECTIONS = [
    ["イントロ", 8, "plain", "light", "main", 0.85],
    ["サビ",     8, "octa",  "full",  "high", 1.00],
];

// ------------------------------------------------------------------ コード
// [表示名, [構成音...], ベースの音]
let Am = ["Am", ["A3", "C4", "E4"], "A2"];
let F  = ["F",  ["F3", "A3", "C4"], "F2"];
let C  = ["C",  ["C4", "E4", "G4"], "C3"];
let G  = ["G",  ["G3", "B3", "D4"], "G2"];

// i-VI-III-VII。解決しないので何周でも回せる、EDM とアンセムの定番。
let prog = [];
for _round in steps(0, 4, 1) {
    prog.push(Am); prog.push(F); prog.push(C); prog.push(G);
}
let CHORDS = #{};
for i in steps(0, prog.len(), 1) {
    CHORDS["" + (i + 1)] = prog[i];
}

// 通し小節 -> 半音いくつ上げるか。ドラムは対象外。
let TRANSPOSE = #{};

// ------------------------------------------------------------------ 旋律
// bar() は長さの合計が 16 でなければその場で止める。書き間違いが
// 曲を鳴らす前に見つかる。
let MELODY = #{
    // イントロ。和音の3度・5度を置くだけ。速い曲ほど旋律は単純にする。
    "1": bar([[4,"A4"], [4,"C5"], [8,"E5"]]),
    "2": bar([[4,"A4"], [4,"C5"], [8,"F5"]]),
    "3": bar([[4,"G4"], [4,"C5"], [8,"E5"]]),
    "4": bar([[4,"B4"], [4,"D5"], [8,"G5"]]),
    "5": bar([[4,"A4"], [4,"C5"], [8,"E5"]]),
    "6": bar([[4,"A4"], [4,"C5"], [8,"F5"]]),
    "7": bar([[2,"C5"], [2,"B4"], [2,"A4"], [2,"G4"], [8,"E5"]]),
    "8": bar([[4,"D5"], [4,"B4"], [8,"A4"]]),

    // サビ。1オクターブ上げて、到達点を作る。
    "9":  bar([[3,"A5"], [1,"A5"], [2,"E5"], [2,"A5"], [4,"C6"], [4,"B5"]]),
    "10": bar([[3,"A5"], [1,"A5"], [2,"F5"], [2,"A5"], [4,"C6"], [4,"A5"]]),
    "11": bar([[3,"G5"], [1,"G5"], [2,"E5"], [2,"G5"], [4,"C6"], [4,"G5"]]),
    "12": bar([[3,"B5"], [1,"B5"], [2,"G5"], [2,"D5"], [8,"G5"]]),
    "13": bar([[3,"A5"], [1,"A5"], [2,"E5"], [2,"A5"], [4,"C6"], [4,"B5"]]),
    "14": bar([[3,"A5"], [1,"A5"], [2,"F5"], [2,"A5"], [4,"C6"], [4,"A5"]]),
    "15": bar([[2,"C6"], [2,"B5"], [2,"A5"], [2,"G5"], [4,"E5"], [4,"G5"]]),
    "16": bar([[4,"D6"], [4,"B5"], [8,"A5"]]),
};

// ------------------------------------------------------------------ 編成
// どの小節で何を鳴らすか。ここに無いパートはその小節で黙る。
let sabi = ["lead", "chords", "bass", "sub", "arp",
            "kick", "clap", "closedhat", "openhat", "shaker"];

let ARRANGE = #{
    // イントロはだんだん増やす。頭から全部鳴らすと盛り上がる場所が無くなる。
    "1": ["lead"],
    "2": ["lead"],
    "3": ["lead", "arp"],
    "4": ["lead", "arp"],
    "5": ["lead", "arp", "bass", "sub", "kick", "closedhat"],
    "6": ["lead", "arp", "bass", "sub", "kick", "closedhat"],
    "7": ["lead", "arp", "bass", "sub", "chords", "kick", "clap", "closedhat"],
    "8": ["lead", "arp", "bass", "sub", "chords", "kick", "clap", "closedhat"],
};
for i in steps(9, 17, 1) {
    ARRANGE["" + i] = sabi;
}

// 小節単位のベロシティ倍率。8小節すべて同じ強さだと平らに聞こえる。
// 一度引いてから開けると溜まる。
let BAR_ACCENT = #{
    "9": 1.00, "10": 0.90, "11": 0.96, "12": 1.04,   // サビ前半: 引いて溜める
    "13": 1.06, "14": 0.96, "15": 1.04, "16": 1.14,  // サビ後半: 開ける
};

// ------------------------------------------------------------------ 伴奏の型
// コード: [16分位置, 長さ, ベロシティ]
let CHORD_PATTERN = [[2,1,84], [6,1,76], [10,1,84], [14,1,80]];

// リフ: [16分位置, 長さ, 和音の何番目の音か, 何オクターブ上げるか]
// ここが Rhai を選んだ理由。「2つおきに和音の音を順に」が1行で書ける。
let ARP_PATTERNS = #{};
let main_arp = [];
let high_arp = [];
for i in steps(0, 16, 2) {
    main_arp.push([i, 2, (i / 2) % 3, 0]);
    high_arp.push([i, 2, (i / 2) % 3, 1]);
}
ARP_PATTERNS.main = main_arp;
ARP_PATTERNS.high = high_arp;

// ベース: [16分位置, 長さ, ルートからの半音差]
let BASS_PATTERNS = #{};
let plain = [];
for i in steps(0, 16, 4) { plain.push([i, 4, 0]); }
// オクターブベース。根音と1オクターブ上を同時に叩いて刻む
let octa = [];
for i in steps(0, 16, 2) {
    octa.push([i, 2, 0]);
    octa.push([i, 2, 12]);
}
BASS_PATTERNS.plain = plain;
BASS_PATTERNS.octa = octa;

// ------------------------------------------------------------------ ドラム
// キット名 -> パート名 -> [MIDIノート番号, [[16分位置, ベロシティ], ...]]
let four = [];
for i in steps(0, 16, 4) { four.push([i, 100]); }
let four_hard = [];
for i in steps(0, 16, 4) { four_hard.push([i, 126]); }
let off_hat = [];
for i in steps(2, 16, 4) { off_hat.push([i, 52]); }
let eighth_hat = [];
for i in steps(0, 16, 2) { eighth_hat.push([i, 60]); }
let shake = [];
for i in steps(0, 16, 2) { shake.push([i, 44]); }
let shake_hard = [];
for i in steps(0, 16, 2) { shake_hard.push([i, 48]); }

let DRUM_KITS = #{
    light: #{
        kick:      [36, four],
        closedhat: [42, off_hat],
        openhat:   [46, []],
        clap:      [39, []],
        shaker:    [70, shake],
        rim:       [37, [[4,70], [12,70]]],
    },
    full: #{
        kick:      [36, four_hard],
        clap:      [39, [[4,112], [12,112]]],
        closedhat: [42, eighth_hat],
        openhat:   [46, [[6,78], [14,78]]],
        shaker:    [70, shake_hard],
        rim:       [37, []],
    },
};

// キットに依らない一発もの
let EXTRA_HITS = #{
    crash: [49, [[0,112]]],
    ghost: [38, [[7,44], [15,52]]],
    fill:  [38, [[8,86], [10,94], [12,104], [14,118], [15,127]]],
};

// ------------------------------------------------------------------ 音色
// patch は自作シンセの音色名。46 種類ある。
// program は GM 音源へ書き出すときの予備。
let VOICES = #{
    lead:   #{ ch: 0,  patch: "supersaw", program: 81,  volume: 100,
               label: "主旋律",     color: "#ff9f43" },
    chords: #{ ch: 1,  patch: "stab",     program: 62,  volume: 82,
               label: "コード",     color: "#9c88ff" },
    bass:   #{ ch: 2,  patch: "acid",     program: 38,  volume: 104,
               label: "ベース",     color: "#26de81" },
    // サブベース。40〜90Hz を持続音で支える土台。bass はその上で刻む。
    // 1本で両方やろうとすると必ずどちらかが足りなくなる。
    sub:    #{ ch: 5,  patch: "sub",      program: 39,  volume: 110,
               label: "サブベース", color: "#00b894" },
    arp:    #{ ch: 3,  patch: "pluck",    program: 80,  volume: 106,
               label: "アルペジオ", color: "#54a0ff" },
    fx:     #{ ch: 4,                     program: 119, volume: 96,
               label: "効果音",     color: "#b2bec3" },
    drums:  #{ ch: 9,  volume: 112, label: "ドラム",   color: "#fd79a8" },
    // MIDI では9番だけが打楽器扱い。perc も9番のままにしておく
    perc:   #{ ch: 9,  volume: 100, label: "パーカス", color: "#54a0ff" },
    vocal:  #{ ch: 10, volume: 100, label: "歌",       color: "#ff4d6d" },
};

let EDIT_PARTS = ["lead", "arp", "chords", "bass", "sub", "drums", "perc", "vocal"];

// 外の歌声ソフト（Synthesizer V など）で書き出した WAV を置くトラック。
// パスは TONESCRIPT_ROOT からの相対で書くこと。絶対パスは受け付けない
// （書いた本人の PC でしか動かなくなるため）。
let AUDIO_TRACKS = #{
    // main:    #{ path: "Vocal/main.wav",    gain: 1.00, label: "メイン" },
    // double:  #{ path: "Vocal/double.wav",  gain: 0.85, label: "重ね" },
    // harmony: #{ path: "Vocal/harmony.wav", gain: 0.42, label: "ハモリ" },
};

// ------------------------------------------------------------------ 主旋律
// セクションごとに楽器を差し替える
let SECTION_PATCH = #{
    "イントロ": #{ lead: "piano", arp: "pluck", bass: "sub", chords: "strings" },
};
let LEAD_PATCH = #{ "イントロ": "piano" };

// 主音 A の自然短音階（A B C D E F G）
let SCALE = [0, 2, 3, 5, 7, 8, 10];
let SCALE_ROOT = 9;               // A のピッチクラス

// ------------------------------------------------------------------ ミックス
let GAINS = #{
    lead: 1.55, chords: 1.60, bass: 0.90, arp: 1.00,
    drums: 1.55, perc: 1.20, fx: 1.40, vocal: 2.30, sub: 1.60,
};
let MASTER_GAIN = 1.0;

// width は左右の広がり（0=中央）、reverb は残響の送り量、
// duck はサイドチェインの掛かり具合。
// 低音を広げると芯がぼやけるので bass と sub は中央に置く。
let MIX = #{
    lead:   #{ width: 1.35, reverb: 0.26, duck: 0.55 },
    arp:    #{ width: 1.60, reverb: 0.30, duck: 0.90 },
    chords: #{ width: 1.50, reverb: 0.34, duck: 1.00 },
    bass:   #{ width: 0.00, reverb: 0.02, duck: 1.00 },
    sub:    #{ width: 0.00, reverb: 0.00, duck: 1.00 },
    drums:  #{ width: 0.45, reverb: 0.08, duck: 0.00 },
    // 金物は広げてよい。低音を含まないので広げても芯はぼやけない。
    perc:   #{ width: 0.85, reverb: 0.14, duck: 0.35 },
    fx:     #{ width: 1.20, reverb: 0.30, duck: 0.30 },
    vocal:  #{ width: 0.00, reverb: 0.22, duck: 0.40 },
};

// サイドチェイン: [凹む深さ, アタック秒, 保持秒, 戻り秒]
// EDM の「ポンプ感」の正体。キックのたびに他を凹ませ、
// 戻る途中で次のキックが来る。
let SIDECHAIN = [0.70, 0.003, 0.020, 0.200];

// キックの作り方
let KICK = #{ weight: 1.15, body: 1.20, click: 0.85, length: 1.15, tail_hz: 78.0 };

// 残響 [秒, 広がり]
let REVERB = [1.9, 4.2];

// 音圧の目標（LUFS）
let MASTER_LUFS = -9.0;
let PREMIX_LUFS = -20.0;
