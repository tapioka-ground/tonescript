# The song file

Full spec for `songs/*.rhai`. Write one by hand, or hand this file to an AI
(Claude Code, ChatGPT) and let it write one.

日本語版は [SONGFILE.ja.md](SONGFILE.ja.md)。

Check your work:

```
tone check  <song>    does it load, how many notes per part
tone render <song>    make the WAV
```

---

## 0. The smallest song

```rhai
let BPM = 120;
let SECTIONS = [["A", 2, "p", "k", "m", 1.0]];
let VOICES = #{ lead: #{ ch: 0, patch: "piano" } };
let MELODY = #{
    "1": bar([[4, "C4"], [4, "E4"], [8, "G4"]]),
    "2": bar([[16, "C5"]]),
};
let ARRANGE = #{ "1": ["lead"], "2": ["lead"] };
```

C, E, G, C on a piano. Only `BPM`, `SECTIONS` and `VOICES` are required.

---

## 1. Rules that matter most

### The unit is one sixteenth note

Every position and length is counted in sixteenths. Call it a **step**.

| Note | Steps |
|---|---|
| 16th | 1 |
| 8th | 2 |
| dotted 8th | 3 |
| quarter | 4 |
| dotted quarter | 6 |
| half | 8 |
| whole (one 4/4 bar) | 16 |

### Steps per bar depend on the meter

```
steps per bar = numerator × 16 ÷ denominator
```

| Meter | Bar | Beat |
|---|---|---|
| 4/4 | 16 | 4 |
| 3/4 | 12 | 4 |
| 2/4 | 8 | 4 |
| 5/4 | 20 | 4 |
| 6/8 | 12 | 2 |
| 7/8 | 14 | 2 |
| 12/8 | 24 | 2 |

**BPM counts beats.** In 6/8, BPM 120 means 120 eighth notes per minute.

### Positions are bar numbers, not offsets

`MELODY` and `CHORDS` are maps keyed by bar number, counting from 1.
You never count steps from the start of the song. The one exception is
`AUTOMATION`, which uses absolute steps.

### Rhai gotchas

- Quote non-ASCII keys: `#{ "イントロ": ... }`
- Quote numeric keys too: `#{ "1": ..., "2": ... }`
- `_` is not a variable name. Write `for _round in ...`
- Array length is `arr.len()`, not `arr.len`

---

## 2. Helpers

Three functions exist inside a song file.

### `bar([[len, name], ...])`

Builds one bar of melody. **The lengths must add up to exactly one bar**,
or loading stops. Mistakes surface before you hear them.

```rhai
bar([[4, "C4"], [4, "E4"], [8, "G4"]])   // 4+4+8 = 16, fine in 4/4
```

Use `"rest"` as the name for a rest.

```rhai
bar([[4, "C4"], [4, "rest"], [8, "G4"]])
```

A raw array `[[4, "C4"], ...]` works too. Either way the sum is checked
against the meter.

### `note("A4")`

Note name to MIDI number. `A4` = 69, `C4` = 60. `#` and `b` both work
(`C#4` and `Db4` are both 61).

### `steps(from, to, by)`

Same as Python's `range`. The end is excluded.

```rhai
for i in steps(0, 16, 2) { ... }   // 0, 2, 4, 6, 8, 10, 12, 14
```

**This is why the song file is a script and not a data format.** "Every
other step" is one line. As data it would be a wall of numbers with the
intent lost.

---

## 3. Required

### `BPM` (number)

Tempo, **20–400**. Out of range is refused at load.

### `SECTIONS` (array)

The skeleton, in order.

```rhai
let SECTIONS = [
    // [name, bars, bass pattern, drum kit, arp pattern, gain, meter]
    ["Intro",  8, "plain", "light", "main", 0.85],
    ["Chorus", 8, "octa",  "full",  "high", 1.00],
    ["Break",  4, "plain", "light", "main", 0.90, [7, 8]],
];
```

| # | Meaning | Range |
|---|---|---|
| 1 | Name (string) | `SECTION_PATCH` looks it up, so keep spellings identical |
| 2 | Bars | 1–512 |
| 3 | Bass pattern name | a key of `BASS_PATTERNS` |
| 4 | Drum kit name | a key of `DRUM_KITS` |
| 5 | Arp pattern name | a key of `ARP_PATTERNS` |
| 6 | Velocity scale | roughly 0.6–1.2 |
| 7 | Meter `[num, den]` (**optional**) | num 1–32, den 1/2/4/8/16. Omitted = 4/4 |

A name that isn't in `BASS_PATTERNS` etc. makes **that part silent** — it
is not an error. When something doesn't sound, check here first.

### `VOICES` (map)

One entry per part.

```rhai
let VOICES = #{
    lead:  #{ ch: 0, patch: "supersaw", program: 81, volume: 100,
              label: "Lead", color: "#ff9f43" },
    drums: #{ ch: 9, volume: 112, label: "Drums", color: "#fd79a8" },
};
```

| Key | Meaning | Default |
|---|---|---|
| `ch` | MIDI channel 0–15. **9 is percussion** | required |
| `patch` | synth instrument name (46 of them, §9) | none = silent |
| `program` | GM program 0–127, used for MIDI export | not sent |
| `volume` | 0–127 | 100 |
| `label` | shown in the window and in MIDI | the part name |
| `color` | `#rrggbb` | grey |

**Part names are fixed.** Generation only happens for these names.

| Part | What it plays |
|---|---|
| `lead` | `MELODY` |
| `bass` | `CHORDS` root × `BASS_PATTERNS` |
| `sub` | `CHORDS` root an octave down, held for the whole bar |
| `arp` | `CHORDS` tones × `ARP_PATTERNS` |
| `chords` | `CHORDS` tones × `CHORD_PATTERN` |
| `drums` | `DRUM_KITS` and `EXTRA_HITS` |
| `perc` | the metal of those (`closedhat` `openhat` `shaker` `ride` `crash` `reverse`) |
| `vocal` | nothing generated. Draw it in the window or import MIDI |
| `fx` | same |

---

## 4. The notes

### `MELODY` — the tune

Bar number → that bar's melody. `lead` plays it.

```rhai
let MELODY = #{
    "1": bar([[3, "A5"], [1, "A5"], [2, "E5"], [2, "A5"], [4, "C6"], [4, "B5"]]),
    "2": bar([[16, "rest"]]),                      // a whole bar of silence
    "3": bar([[4, "C5", "la"], [12, "E5", "laa"]]), // 3rd item = lyric (optional)
};
```

- The sum must equal **that bar's step count**. Otherwise loading stops with
  something like `bar 3: melody sums to 16/12 (meter 3/4)`
- A bar number past the end of the song also stops loading
- Velocity starts at 108, then section gain and `BAR_ACCENT` multiply it

**You get the pitches you wrote.** Nothing is transposed or snapped to a
chord (only `TRANSPOSE` moves anything).

### `CHORDS` — harmony

Bar number → chord. Feeds `bass`, `sub`, `arp` and `chords`.

```rhai
let Am = ["Am", ["A3", "C4", "E4"], "A2"];
//         name   tones (upper parts)   root (bass and sub)
let CHORDS = #{ "1": Am, "2": Am };
```

Any number of tones. `arp` picks them by index, so the order matters.

To loop a progression, use `steps`:

```rhai
let prog = [Am, F, C, G];
let CHORDS = #{};
for i in steps(0, 16, 1) {
    CHORDS["" + (i + 1)] = prog[i % prog.len()];
}
```

### `ARRANGE` — what plays in which bar

Bar number → array of parts. **A part not listed is silent in that bar.**

```rhai
let ARRANGE = #{
    "1": ["lead"],
    "2": ["lead", "arp"],
    "3": ["lead", "arp", "bass", "sub", "kick", "closedhat"],
};
```

Drums go in by **hit name, not part name** (`kick`, `clap`, `closedhat`,
`openhat`, `shaker`, `rim` — whatever you named in `DRUM_KITS`).

**No `ARRANGE` means no sound at all.** This is the number one cause of
silence.

Build energy by adding parts:

```rhai
let full = ["lead", "chords", "bass", "sub", "arp",
            "kick", "clap", "closedhat", "openhat", "shaker"];
let ARRANGE = #{
    "1": ["lead"],
    "2": ["lead"],
    "3": ["lead", "arp"],
    "4": ["lead", "arp"],
    "5": ["lead", "arp", "bass", "sub", "kick", "closedhat"],
};
for i in steps(6, 17, 1) { ARRANGE["" + i] = full; }
```

---

## 5. Accompaniment patterns

### `BASS_PATTERNS`

Name → array of `[pos, len, semitones from root]`. One bar's worth.

```rhai
let BASS_PATTERNS = #{
    // on every quarter
    plain: [[0, 4, 0], [4, 4, 0], [8, 4, 0], [12, 4, 0]],
    // root and octave together
    octa: [[0,2,0],[0,2,12],[2,2,0],[2,2,12],[4,2,0],[4,2,12],[6,2,0],[6,2,12],
           [8,2,0],[8,2,12],[10,2,0],[10,2,12],[12,2,0],[12,2,12],[14,2,0],[14,2,12]],
};
```

The offset is from the `CHORDS` root: `12` an octave up, `7` a fifth,
`-12` an octave down. Velocity starts at 100.

**Hits past the end of a short bar are dropped.** In 3/4 (12 steps), a
pattern with `[12, 4, 0]` loses that hit. Nothing spills into the next bar.

### `ARP_PATTERNS`

Name → array of `[pos, len, which chord tone, octaves up]`.

```rhai
let ARP_PATTERNS = #{
    main: [[0,2,0,0],[2,2,1,0],[4,2,2,0],[6,2,0,0],
           [8,2,1,0],[10,2,2,0],[12,2,0,0],[14,2,1,0]],
};
```

"Which chord tone" indexes `CHORDS` tones from 0. An index past the end
wraps rather than dropping. Velocity starts at 92.

### `CHORD_PATTERN`

Array of `[pos, len, velocity]`. **One per song, no names.** All chord
tones at once.

```rhai
let CHORD_PATTERN = [[2, 1, 84], [6, 1, 76], [10, 1, 84], [14, 1, 80]];
```

Short hits on the offbeats (2, 6, 10, 14) is the usual move.

### `sub` needs no pattern

Put `sub` in `ARRANGE` and it takes the `CHORDS` root an octave down and
holds it for the bar. Velocity starts at 96.

---

## 6. Drums

### `DRUM_KITS`

Kit name → hit name → `[MIDI note, [[pos, velocity], ...]]`

```rhai
let DRUM_KITS = #{
    light: #{
        kick:      [36, [[0,100],[4,100],[8,100],[12,100]]],
        closedhat: [42, [[2,52],[6,52],[10,52],[14,52]]],
        clap:      [39, []],                      // empty is fine
    },
    full: #{
        kick:      [36, [[0,126],[4,126],[8,126],[12,126]]],
        clap:      [39, [[4,112],[12,112]]],
        closedhat: [42, [[0,60],[2,60],[4,60],[6,60],[8,60],[10,60],[12,60],[14,60]]],
    },
};
```

**The MIDI note picks the sound.** The hit name is just a label for
`ARRANGE` to point at.

| Note | Sound |
|---|---|
| 35 | distorted kick (hardstyle) |
| 36 | kick |
| 37 | rimshot |
| 38 | snare |
| 39 | clap |
| 41 / 45 / 48 | tom (low / mid / high) |
| 42 | closed hat |
| 46 | open hat |
| 49 | crash |
| 51 | ride |
| 52 | reverse cymbal |
| 70 | shaker |
| anything else | hat |

### `EXTRA_HITS` — one-offs outside the kit

```rhai
let EXTRA_HITS = #{
    crash: [49, [[0, 112]]],
    fill:  [38, [[8,86],[10,94],[12,104],[14,118],[15,127]]],
};
```

They fire in bars where `ARRANGE` names them.

### How hits split between `drums` and `perc`

`closedhat`, `openhat`, `shaker`, `ride`, `crash` and `reverse` go to the
`perc` lane; everything else to `drums`, so the two can be mixed apart.
In `ARRANGE` you just write hit names — the lane is automatic.

---

## 7. Making it move

### `TEMPO_MAP`

Bar number → BPM, interpolated in between.

```rhai
let TEMPO_MAP = #{ "1": 150, "33": 174, "41": 190, "49": 190, "52": 154, "53": 150 };
let TEMPO_CURVE = "smooth";   // "smooth" (default) or "linear"
```

- `smooth` is flat at both ends: it eases out and eases in
- `linear` is a straight line, so the landing has a corner

**Dropping over a few bars beats stopping dead** — the next full section
hits harder.

### `TRANSPOSE`

Bar number → semitones up. **Drums are never transposed.**

```rhai
let TRANSPOSE = #{};
for i in steps(53, 69, 1) { TRANSPOSE["" + i] = 2; }   // last chorus up a tone
```

### `BAR_ACCENT`

Bar number → multiplier. Eight bars at one level sounds flat.

```rhai
let BAR_ACCENT = #{
    "9": 1.00, "10": 0.90, "11": 0.96, "12": 1.04,   // pull back, build up
    "13": 1.06, "14": 0.96, "15": 1.04, "16": 1.14,  // open up
};
```

### `SECTION_PATCH` / `LEAD_PATCH`

```rhai
let SECTION_PATCH = #{
    "Intro":  #{ lead: "koto", arp: "koto", bass: "sub", chords: "strings" },
    "Chorus": #{ lead: "hardlead", bass: "hardbass" },
};
let LEAD_PATCH = #{ "Intro": "piano" };   // shortcut for lead only
```

Priority: `SECTION_PATCH` → `LEAD_PATCH` → `VOICES.patch`.

### `AUTOMATION`

Part → lane → `[[pos, value], ...]`. **Positions here are absolute steps**,
not bars. Straight lines between points.

```rhai
let AUTOMATION = #{
    lead: #{
        pan:  [[0, -1.0], [256, 1.0]],             // left to right
        gain: [[0, 1.0], [128, 1.0], [256, 0.25]], // fade out in the back half
    },
};
```

| Lane | Range | Default |
|---|---|---|
| `gain` | 0–8 | 1.0 |
| `pan` | -1 to 1 (-1 is left) | 0 (centre) |
| `reverb` | 0–2 | the `MIX` value |
| `duck` | 0–2 | the `MIX` value |

Out-of-range values, and positions past the end of the song, are refused
at load.

---

## 8. Mix and master

```rhai
let GAINS = #{ lead: 1.55, chords: 1.60, bass: 0.90, arp: 1.00,
               drums: 1.55, perc: 1.20, fx: 1.40, vocal: 2.30, sub: 1.60 };
let MASTER_GAIN = 1.0;

let MIX = #{
    lead:   #{ width: 1.35, reverb: 0.26, duck: 0.55 },
    bass:   #{ width: 0.00, reverb: 0.02, duck: 1.00 },
    drums:  #{ width: 0.45, reverb: 0.08, duck: 0.00 },
};

let SIDECHAIN = [0.70, 0.003, 0.020, 0.200];   // [depth, attack s, hold s, release s]
let KICK = #{ weight: 1.15, body: 1.20, click: 0.85, length: 1.15, tail_hz: 78.0 };
let REVERB = [1.9, 4.2];                        // [seconds, spread]
let MASTER_LUFS = -9.0;                         // final loudness
let PREMIX_LUFS = -20.0;
```

| `MIX` key | Meaning |
|---|---|
| `width` | stereo width. 0 is dead centre. **Widening the low end blurs it**, so keep `bass` and `sub` at 0 |
| `eq` | three bands of tone shaping (below) | flat |
| `reverb` | send amount. 0 sends nothing. 0.2–0.4 is a normal range |
| `duck` | how much the sidechain pushes this part down on every kick |

### `eq` — taking space away from one part to give it to another

A mix is not built with volume alone. Bass and kick both live down low, so
whatever you do to their faders they keep covering each other. **The fix is
to cut one of them where the other needs room.**

```rhai
let MIX = #{
    bass:  #{ eq: #{ low: 2.0, mid: -3.0 } },
    drums: #{ eq: #{ low: -4.0, high: 3.0 } },
};
```

| Key | Meaning | Default |
|---|---|---|
| `low` | dB applied below `low_hz`, −24 to 24 | 0 |
| `mid` | dB applied around `mid_hz` | 0 |
| `high` | dB applied above `high_hz` | 0 |
| `low_hz` | where the low shelf turns | 200 |
| `mid_hz` | centre of the mid bump | 1000 |
| `mid_q` | how narrow the mid is, 0.2–12 | 0.9 |
| `high_hz` | where the high shelf turns | 4000 |

Three bands is enough. Full desks carry six or eight, but what actually
gets used is "take the bottom off", "push or pull the middle", "add air".

**Cutting beats boosting.** If a part is dull, first try cutting the middle
of whatever is on top of it. Everything you boost also eats headroom.

### Using the reverb

`REVERB` is `[seconds, spread]`. Seconds is roughly how long the tail takes
to die (0.05–20); spread is 0 for dead centre, 8 for widest.

**Every part sends to the same room.** Rather than a separate reverb per
part, one room takes the sends and its output is added back. That is how a
real room works, and it sits better — separate reverbs make parts sound
like they are in different places.

`MIX`'s `reverb` sets the send. **At 0 nothing is sent**, and if every part
is 0 the reverb is never computed at all.

```rhai
let REVERB = [2.5, 5.0];
let MIX = #{
    lead:   #{ width: 1.35, reverb: 0.26, duck: 0.55 },
    // Don't send the low end. Reverb only smears its edge.
    bass:   #{ width: 0.00, reverb: 0.02, duck: 1.00 },
    sub:    #{ width: 0.00, reverb: 0.00, duck: 1.00 },
};
```

The longer the tail, the sooner the highs fall out of it — as in a real room.

`SIDECHAIN` is what makes EDM pump: every kick pushes everything else down,
and the next kick lands before it has recovered. Deeper and slower to
recover means more swell.

`MASTER_LUFS`: -14 for streaming, around -9 for CD.

### `AUDIO_TRACKS` — vocals recorded elsewhere

```rhai
let AUDIO_TRACKS = #{
    main:    #{ path: "Vocal/main.wav",    gain: 1.00, label: "Main" },
    double:  #{ path: "Vocal/double.wav",  gain: 0.85, label: "Double" },
    harmony: #{ path: "Vocal/harmony.wav", gain: 0.42, label: "Harmony",
                at: 64, trim_in: 0.20, fade_out: 1.5 },
};
```

| Key | Meaning | Default |
|---|---|---|
| `path` | relative to `TONESCRIPT_ROOT`. Absolute paths are refused | required |
| `gain` | 0 to skip it entirely | 1.0 |
| `label` | shown in the window | the name |
| `at` | which step it starts on. 0 is the top of the song | 0 |
| `trim_in` | seconds cut off the front (a breath, a chair creak) | 0 |
| `trim_out` | seconds cut off the end | 0 |
| `fade_in` | seconds to fade up | 0 |
| `fade_out` | seconds to fade down | 0 |

- **48kHz, 16/24-bit or 32-bit float, mono or stereo.** Anything else is an error
- Seconds are 0–600. `at` is a step, so it follows tempo changes
- Layering two takes of the same part is doubling, and it thickens
- Set `gain: 0.0` for a track you aren't using
- Vocals get neither the sidechain nor the crest limiter

### `SCALE` / `SCALE_ROOT`

The scale, in semitones from the root. Used when building parallel parts.

```rhai
let SCALE = [0, 2, 3, 5, 7, 8, 10];   // natural minor
let SCALE_ROOT = 9;                    // A (C=0, A=9)
```

---

## 9. The 135 instruments

`tone patches` prints them. They come in two kinds, and both are used the
same way — just a name in `VOICES`.

**Synth voices (46)** — written as code, tuned by hand.

| Family | Names |
|---|---|
| Lead | `supersaw` `hardlead` `brightsaw` `squarelead` `pluck` `stab` `crystal` `brass` |
| Keys, strings | `piano` `harpsi` `organ` `strings` `choir` |
| Bass | `acid` `sub` `reese` `fmbass` `donk` `808` `growl` `hardbass` `fingerbass` `upright` `rumble` `wobble` |
| Tuned percussion | `bell` `steelpan` `marimba` `kalimba` `gamelan` `santur` |
| East Asian | `koto` `shamisen` `shakuhachi` `sinobue` `erhu` |
| Winds | `flute` `quena` `bansuri` `ocarina` `whistle` `panflute` `duduk` `didge` |
| Plucked | `sitar` `oud` |


**Ordinary instruments (89)** — written as recipes (§10), so you can copy
one into your song file and change it.

| Family | Names |
|---|---|
| Plucked strings | `guitar` `nylon` `eguitar` `distguitar` `twelvestring` `ukulele` `banjo` `mandolin` `balalaika` `bouzouki` `charango` `harp` `kora` `dulcimer` `pizzicato` |
| Bass | `ebass` `slapbass` `fretless` |
| Bowed | `violin` `viola` `cello` `contrabass` `tremolostrings` `kokyu` |
| Japanese | `kotostring` `shamisenstring` `biwa` `sanshin` `koto12` `sho` `hichiriki` |
| Asian | `guzheng` `pipa` `sitarstring` |
| Keys | `rhodes` `wurli` `clav` `celesta` `musicbox` `toypiano` `harmonium` `leslie` `melodica` `accordion` `harmonica` |
| Tuned percussion | `vibraphone` `glocken` `xylophone` `tubularbell` `handpan` `steeldrum` `logdrum` `woodblock` `timpani` `taiko` |
| Brass | `trumpet` `mutetrumpet` `trombone` `horn` `tuba` |
| Woodwind | `sax` `clarinet` `oboe` `bassoon` `piccolo` `recorder` `bagpipe` |
| Voice | `aah` `ooh` `hum` `whisper` |
| Pads | `warmpad` `glasspad` `choirpad` `sweeppad` `stringmachine` `bellpad` |
| Synth | `plucksynth` `bellsynth` `pwmlead` `hoover` `organbass` `clickbass` |
| Chiptune | `nespulse` `nesthin` `neslead` `nestri` `nesarp` `gameboy` |

**How close they get, honestly.** The plucked strings model the actual
string — a wave running up and down it, losing its highs at each end — so
they hold up on their own. Keys and tuned metal come out well, because
their overtones are simple to begin with. The winds are *plausible*, not
real: a real one is breath and lips fighting a tube, which isn't modelled
here. Bowed strings are the furthest off.

Drums, piano and orchestra recorded from life are **not** in reach of any
of this, which is why sample libraries are tens of gigabytes.

Rough guidance for the synth voices:

- **Fast lead** — `supersaw`, `hardlead`, `brightsaw`. Rich enough to cut through
- **Quiet passages** — `piano`, `crystal`, `strings`. `crystal` clears as it sustains
- **Low end** — `sub` holds 40–90Hz while `acid` or `reese` moves on top
- **Japanese** — `koto` (plucked, distinct attack), `shakuhachi` (breath is the point), `shamisen` (buzzing sawari)
- **Breath instruments** are slow to start. Not for fast passages

Some instruments **ring past the note length**: `gamelan` 1.6s, `piano`
0.42s, `808` 0.70s. That's why fast passages on them run together.

---

---

## 10. Making your own instruments

The 46 built-ins are not the limit. `PATCHES` builds an instrument out of
numbers, and it can be used anywhere a built-in name can — including in
`VOICES`, `SECTION_PATCH` and `LEAD_PATCH`.

**A name you define beats the built-in of the same name.** Don't like the
built-in `piano`? Write your own `piano`.

The smallest one that makes a sound:

```rhai
let PATCHES = #{ plain: #{} };
```

Everything has a default. Write only what you want to change.

### How it is put together

```text
  oscillators ─┬→ amp envelope ─→ filter ─→ drive ─→ delay ─→ gain
  partials    ─┤       ↑            ↑
  FM          ─┤    vibrato   filter envelope
  attack noise─┘
```

### `osc` — the oscillators

Stack as many as you like (up to 16). `mix` is relative, so `1.0` and `1.0`
means half each.

```rhai
osc: [
    #{ wave: "saw",    mix: 1.0, detune: -14 },
    #{ wave: "saw",    mix: 1.0, detune:  14 },
    #{ wave: "square", mix: 0.5, octave: -1 },
],
```

| Key | Meaning | Default |
|---|---|---|
| `wave` | `saw` / `square` / `sine` / `noise` / `string` | `saw` |
| `mix` | how much of it | 1.0 |
| `detune` | cents off (100 = a semitone). ±4800 | 0 |
| `octave` | octaves up or down. ±4 | 0 |

**Detuning two saws by ±10–20 cents is what makes a "fat" lead.** Any more
and it stops sounding like one note.

### `env` — the shape of the volume

```rhai
env: #{ a: 0.008, d: 0.20, s: 0.75, r: 0.12 },
```

| Key | Meaning |
|---|---|
| `a` | seconds to rise. 0.001 is a hit, 0.3 is a breath |
| `d` | seconds to fall to the sustain level |
| `s` | the level it holds at (0–1). **0 means it dies away** — right for a plucked string |
| `r` | seconds to fade after the note ends |

### `filter` — taking the top off

```rhai
filter: #{ kind: "ladder", base: 700, sweep: 7000, res: 0.35, vel: 1500,
           env: #{ a: 0.004, d: 0.25, curve: 2.2 } },
```

| Key | Meaning | Default |
|---|---|---|
| `kind` | `ladder` (= `lowpass`) / `highpass` / `bandpass` / `none` | `ladder` if you wrote anything else here |
| `base` | Hz at its most closed | 1200 |
| `sweep` | how many Hz the envelope opens it | 4000 |
| `res` | resonance, 0–0.95. **Above 0.8 it starts to whistle** | 0.2 |
| `track` | how much it follows pitch. 1.0 = high notes open further | 0 |
| `vel` | extra Hz when hit hard | 0 |
| `env.a` `env.d` `env.curve` | the shape it opens with | 0.001 / 0.08 / 2.0 |

**`base` low + `sweep` high is the classic "pluck".** The filter slams open
and shuts again.


### `wave: "string"` — a real plucked string

The other waves are shapes. This one is **the string itself**: a burst of
noise runs up and down a tube the length of the string, losing a little of
its top each time it turns around. That is why a real string goes dull as
it dies, and why "a sawtooth through a filter" never quite sounds plucked.

```rhai
osc: [#{ wave: "string", decay: 3.2, bright: 0.62, pick: 0.20 }],
```

| Key | Meaning | Default |
|---|---|---|
| `decay` | seconds it rings. 0.5 is a muted pluck, 5 an open one | 2.0 |
| `bright` | 0–1. Low is nylon and goes dull fast; high is steel and holds | 0.5 |
| `pick` | where you pluck, 0–1. 0.5 is the middle (round), 0.1 near the bridge (hard) | 0.25 |

It ignores `vibrato` and `fm` — you can't wobble a plucked string.

### `body` — the box it is in

`[centre Hz, sharpness, how much]`. Half of what makes a guitar sound like
a guitar is not the string but the **box**. Without it the string is thin.

```rhai
body: [[100.0, 6.0, 0.45], [205.0, 8.0, 0.25], [430.0, 9.0, 0.12]],
```

Those are roughly an acoustic guitar. A ukulele's box is smaller, so its
peaks sit higher (240Hz, 520Hz). An amplifier cabinet is a wide, soft peak
around 400Hz.

### `partials` — inharmonic tones

`[pitch multiple, level, seconds to die]`, added directly. Real bells and
gongs have partials that are **not** whole multiples, which is exactly why
they don't sound like a sawtooth.

```rhai
osc: [],
partials: [[1.0, 0.5, 2.4], [2.76, 0.28, 1.6], [5.40, 0.16, 1.0]],
```

Those three numbers (1, 2.76, 5.4) are roughly a real church bell.

### `attack` — noise at the front

```rhai
attack: #{ amount: 0.22, hp: 2500, a: 0.0003, d: 0.006 },
```

The fingernail on a string, the chiff of a flute. `hp` cuts everything
below it, so the burst sits on top instead of thickening the low end.

### `fm` — metallic

```rhai
fm: #{ ratio: 3.5, index: 6.0, decay: 0.4 },
```

One wave wobbles the pitch of another. `ratio` is whole (2, 3) for musical
results and fractional (3.5, 5.7) for clangy ones. `index` is how hard.

### `vibrato` / `delay` / `drive` / `gain` / `ring`

```rhai
vibrato: #{ rate: 5.2, depth: 0.008, delay: 0.3 },  // rate Hz, depth 0.01 ≈ ±17 cents
delay:   #{ time: 0.12, feedback: 0.4, mix: 0.3 },
drive: 1.6,   // 1.0 = clean. Above 2 it is clearly distorted
gain: 0.55,   // final level
ring: 0.18,   // seconds it keeps sounding after the note ends
```

**`ring` is what makes fast passages run together.** Bells want 1–2s,
a piano 0.4s, a lead 0.

### Recipes to start from

```rhai
let PATCHES = #{
    // Fat lead
    fatsaw: #{
        osc: [#{ wave: "saw", detune: -14 }, #{ wave: "saw", detune: 14 },
              #{ wave: "saw", mix: 0.6, octave: -1 }],
        env: #{ a: 0.008, d: 0.20, s: 0.75, r: 0.12 },
        filter: #{ base: 700, sweep: 7000, res: 0.35, vel: 1500 },
        drive: 1.6, gain: 0.55,
    },
    // Glass bell
    glassbell: #{
        osc: [],
        partials: [[1.0, 0.5, 2.4], [2.76, 0.28, 1.6], [5.40, 0.16, 1.0]],
        env: #{ a: 0.001, d: 2.5, s: 0.0, r: 0.6 },
        gain: 1.2, ring: 1.8,
    },
    // Plucked string
    pickedstring: #{
        osc: [#{ wave: "saw", mix: 0.6 }, #{ wave: "square", mix: 0.4 }],
        env: #{ a: 0.001, d: 0.45, s: 0.0, r: 0.10 },
        filter: #{ base: 1400, sweep: 6000, res: 0.30 },
        attack: #{ amount: 0.22, hp: 2500, a: 0.0003, d: 0.006 },
        ring: 0.18,
    },
    // Breathy pipe — noise through a narrow band is a flute
    airy: #{
        osc: [#{ wave: "noise" }],
        filter: #{ kind: "bandpass", base: 1200, res: 0.6 },
        env: #{ a: 0.08, d: 0.2, s: 0.8, r: 0.15 },
        vibrato: #{ rate: 5.2, depth: 0.008, delay: 0.3 },
        gain: 1.5,
    },
    // Deep sub
    deepsub: #{
        osc: [#{ wave: "sine" }, #{ wave: "sine", mix: 0.3, octave: -1 }],
        env: #{ a: 0.004, d: 0.1, s: 0.9, r: 0.08 },
        filter: #{ base: 140, sweep: 0, res: 0.1 },
        gain: 1.0,
    },
};
```

### Checks the loader makes

Bad numbers stop the load with a reason, so you find out before you listen:

- no sound source at all (`osc` empty with no `partials` and no `attack`)
- every `mix` is 0
- `res` above 0.95 (it would self-oscillate rather than play)
- `env` times outside 0–30s, `drive` outside 0.1–20
- an unknown `wave` or filter `kind` — the message lists the valid ones

`tone check <song>` runs all of it.

### Asking an AI for a sound

This whole section is the interface. Describe the sound in words:

> Following `SONGFILE.md`, add a `PATCHES` entry called `icepad` —
> a cold, slow, breathy pad that opens over about a second.

Then `tone render <song>` and listen. Sound design is "make one, fix it",
and every number here is a number an AI can adjust on being told
"too dull" or "too harsh".

## 11. Common traps

| Symptom | Cause |
|---|---|
| **No sound** | the part isn't in `ARRANGE` |
| bass/arp silent | the `SECTIONS` pattern name isn't in `BASS_PATTERNS` etc. |
| melody silent | no `lead` in `ARRANGE`, or a `MELODY` bar past the end |
| loading stops | a melody bar doesn't sum to one bar. The message shows e.g. `16/12` |
| 4 beats in 3/4 | the pattern assumes 4/4. Hits past the bar are dropped |
| wrong drum sound | the **MIDI note** picks the sound, not the hit name |
| error on a non-ASCII key | it needs quotes: `#{ "イントロ": ... }` |
| tempo change did nothing | you edited `BPM` but left `TEMPO_MAP` alone |
| vocal won't load | not 48kHz/16-bit, or the path is absolute |

---

## 12. Handing this to an AI

Give it this file and ask:

> Follow the spec in `SONGFILE.md` and write `songs/<name>.rhai`.
> \<what you want the song to be\>

Then always check:

```
tone check  <name>    does it load, how many notes per part
tone render <name>    make the WAV
```

If `check` passes, the song is at least structurally sound — bar sums, bar
ranges and value ranges are all verified at load.

Worth telling the AI up front:

- Only `BPM`, `SECTIONS` and `VOICES` are required. Get those loading first
- Forgetting `ARRANGE` means **nothing plays**
- Every melody bar must sum to one bar. `bar()` catches it immediately
- Change the meter and the accompaniment patterns have to change with it
- Instrument names must be one of the 46, or something you defined in
  `PATCHES` (§10). An unknown name **silences the part**
- If none of the 46 fits, **write the instrument** (§10) rather than settling

---

## 13. Everything, in one table

| Name | Required | Shape | Default |
|---|---|---|---|
| `BPM` | ● | number 20–400 | — |
| `SECTIONS` | ● | array | — |
| `VOICES` | ● | map | — |
| `TITLE` | | string | `"untitled"` |
| `KEY` | | string | `""` |
| `TEMPO_MAP` | | bar → BPM | `{1: BPM}` |
| `TEMPO_CURVE` | | `"smooth"` / `"linear"` | `"smooth"` |
| `CHORDS` | | bar → chord | none |
| `MELODY` | | bar → melody | none |
| `ARRANGE` | | bar → parts | none (= silence) |
| `TRANSPOSE` | | bar → semitones | none |
| `BAR_ACCENT` | | bar → multiplier | 1.0 |
| `BASS_PATTERNS` | | name → array | none |
| `ARP_PATTERNS` | | name → array | none |
| `CHORD_PATTERN` | | array | none |
| `DRUM_KITS` | | kit → hits | none |
| `EXTRA_HITS` | | name → hits | none |
| `EDIT_PARTS` | | array of strings | the `VOICES` keys |
| `AUDIO_TRACKS` | | name → track (`at` `trim_in` `trim_out` `fade_in` `fade_out`) | none |
| `GAINS` | | part → multiplier | 1.0 |
| `MASTER_GAIN` | | number | 1.0 |
| `MIX` | | part → settings | centred, no reverb, no ducking |
| `AUTOMATION` | | part → lanes | none |
| `SECTION_PATCH` | | section → part → instrument | none |
| `LEAD_PATCH` | | section → instrument | none |
| `PATCHES` | | name → instrument recipe | none |
| `SCALE` | | array of semitones | none |
| `SCALE_ROOT` | | number | 0 |
| `SIDECHAIN` | | `[depth, a, h, r]` | `[0.70, 0.003, 0.020, 0.200]` |
| `KICK` | | map | the default kick |
| `REVERB` | | `[seconds, spread]` | `[1.9, 4.2]` |
| `MASTER_LUFS` | | number | -9.0 |
| `PREMIX_LUFS` | | number | -20.0 |
