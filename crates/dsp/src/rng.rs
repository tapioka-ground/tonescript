//! numpy の `np.random.default_rng(seed)` と同じ乱数。
//!
//! なぜ自前の乱数ではなく numpy を再現するのか
//! --------------------------------------------
//! 46 ある音色のほぼ全部が、雑音（`noise`）と乱数の初期位相を使っている。
//! 乱数が違うと波形が違ってしまい、「移植でどこか壊れた」のか
//! 「乱数が違うだけ」なのかが区別できない。
//!
//! ここを合わせておくと、音色 1 本ずつを Python と**サンプル単位で**
//! 突き合わせられる。移植の正しさを耳ではなく数字で確かめるための土台。
//!
//! 中身は numpy と同じ 2 段構え。
//!   1. SeedSequence — 種を 4 つの 32bit へ混ぜる
//!   2. PCG64 (XSL-RR 128/64) — そこから 64bit の列を作る

const XSHIFT: u32 = 16;
const INIT_A: u32 = 0x43b0_d7e5;
const MULT_A: u32 = 0x931e_8875;
const INIT_B: u32 = 0x8b51_f9dd;
const MULT_B: u32 = 0x58f3_8ded;
const MIX_MULT_L: u32 = 0xca01_f9dd;
const MIX_MULT_R: u32 = 0x4973_f715;
const POOL_SIZE: usize = 4;

/// numpy の SeedSequence。種を混ぜて、そこから状態を取り出す。
pub struct SeedSequence {
    pool: [u32; POOL_SIZE],
}

fn hashmix(mut value: u32, hash_const: &mut u32) -> u32 {
    value ^= *hash_const;
    *hash_const = hash_const.wrapping_mul(MULT_A);
    value = value.wrapping_mul(*hash_const);
    value ^= value >> XSHIFT;
    value
}

fn mix(x: u32, y: u32) -> u32 {
    let mut result = MIX_MULT_L.wrapping_mul(x).wrapping_sub(MIX_MULT_R.wrapping_mul(y));
    result ^= result >> XSHIFT;
    result
}

impl SeedSequence {
    /// 種ひとつから作る。numpy の `SeedSequence(seed)` と同じ。
    ///
    /// 種は内部で 32bit ずつに割られる。ここで扱うのは
    /// 曲ファイルから来る小さな整数だけなので u64 までで足りる。
    pub fn new(seed: u64) -> Self {
        // numpy は種を 32bit の並びにしてから混ぜる。
        // 上位が 0 なら 1 語、そうでなければ 2 語。
        let entropy: Vec<u32> = if seed >> 32 == 0 {
            vec![seed as u32]
        } else {
            vec![seed as u32, (seed >> 32) as u32]
        };
        let mut pool = [0u32; POOL_SIZE];
        let mut hash_const = INIT_A;
        for (i, slot) in pool.iter_mut().enumerate() {
            let v = entropy.get(i).copied().unwrap_or(0);
            *slot = hashmix(v, &mut hash_const);
        }
        for i_src in 0..POOL_SIZE {
            for i_dst in 0..POOL_SIZE {
                if i_src != i_dst {
                    let h = hashmix(pool[i_src], &mut hash_const);
                    pool[i_dst] = mix(pool[i_dst], h);
                }
            }
        }
        // numpy はこのあと、種の語数が pool より多い場合の追加混ぜを行う。
        // 語数 <= 4 のここでは何も起きないので省いてある。
        Self { pool }
    }

    /// 32bit の状態を n 語ぶん作る。
    pub fn generate_state32(&self, n_words: usize) -> Vec<u32> {
        let mut hash_const = INIT_B;
        let mut out = Vec::with_capacity(n_words);
        for i in 0..n_words {
            let mut data_val = self.pool[i % POOL_SIZE];
            data_val ^= hash_const;
            hash_const = hash_const.wrapping_mul(MULT_B);
            data_val = data_val.wrapping_mul(hash_const);
            data_val ^= data_val >> XSHIFT;
            out.push(data_val);
        }
        out
    }

    /// 64bit の状態を n 語ぶん。32bit を 2 つずつ繋ぐ（下位が先）。
    pub fn generate_state64(&self, n_words: usize) -> Vec<u64> {
        let w32 = self.generate_state32(n_words * 2);
        (0..n_words)
            .map(|i| (w32[2 * i] as u64) | ((w32[2 * i + 1] as u64) << 32))
            .collect()
    }

    pub fn pool(&self) -> [u32; POOL_SIZE] {
        self.pool
    }
}

/// PCG64（XSL-RR 128/64）。numpy の既定の乱数器。
pub struct Pcg64 {
    state: u128,
    inc: u128,
}

const PCG_MULT: u128 = 47_026_247_687_942_121_848_144_207_491_837_523_525;

impl Pcg64 {
    /// numpy の `default_rng(seed)` と同じ状態で作る。
    pub fn new(seed: u64) -> Self {
        let st = SeedSequence::new(seed).generate_state64(4);
        // numpy の C は seed[0] を「上位 64bit」として組む。
        //   s = ((pcg128_t)seed[0] << 64) | seed[1]
        // ここを逆にすると値が全く合わない。
        let initstate = ((st[0] as u128) << 64) | (st[1] as u128);
        let initseq = ((st[2] as u128) << 64) | (st[3] as u128);
        let mut r = Self {
            state: 0,
            inc: (initseq << 1) | 1,
        };
        r.step();
        r.state = r.state.wrapping_add(initstate);
        r.step();
        r
    }

    #[inline]
    fn step(&mut self) {
        self.state = self.state.wrapping_mul(PCG_MULT).wrapping_add(self.inc);
    }

    /// 次の 64bit。
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.step();
        // XSL-RR: 上下 64bit を xor して、上位 6bit で回す
        let v = self.state;
        let xored = ((v >> 64) as u64) ^ (v as u64);
        let rot = (v >> 122) as u32;
        xored.rotate_right(rot)
    }

    /// 0 以上 1 未満の倍精度。numpy の `random()` と同じ作り方。
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// numpy の `uniform(low, high, n)`。
    pub fn uniform(&mut self, low: f64, high: f64, n: usize) -> Vec<f64> {
        let span = high - low;
        (0..n).map(|_| low + span * self.next_f64()).collect()
    }
}

/// 雑音。Python 版の `noise(n, seed)` と同じ。
pub fn noise(n: usize, seed: u64) -> Vec<f32> {
    let mut rng = Pcg64::new(seed);
    (0..n).map(|_| (-1.0 + 2.0 * rng.next_f64()) as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_sequence_pool_matches_numpy() {
        // numpy: np.random.SeedSequence(seed).pool
        assert_eq!(
            SeedSequence::new(0).pool(),
            [0xfe40eb07, 0x4f363a36, 0x4eb2009d, 0xc89a7aa7]
        );
        assert_eq!(
            SeedSequence::new(1).pool(),
            [0xa137e185, 0x0de2fab1, 0x950eae78, 0x54dd0a81]
        );
        assert_eq!(
            SeedSequence::new(42).pool(),
            [0x631d3606, 0x07ae90ae, 0x6fc4be28, 0x2ced5c75]
        );
    }

    #[test]
    fn generate_state_matches_numpy() {
        // numpy: SeedSequence(seed).generate_state(4, dtype=np.uint32)
        assert_eq!(
            SeedSequence::new(0).generate_state32(4),
            vec![0xb0f478be, 0xdb2cd7e7, 0x2c71ba49, 0xabf4641a]
        );
        assert_eq!(
            SeedSequence::new(1).generate_state32(4),
            vec![0x6d6791ff, 0x672d8ee5, 0x4eb1072c, 0x8ae19ca1]
        );
        assert_eq!(
            SeedSequence::new(42).generate_state32(4),
            vec![0xcd540ab7, 0x9f1e2e6d, 0x79fb94b6, 0xd57873dc]
        );
    }

    #[test]
    fn raw_u64_matches_numpy() {
        // numpy: PCG64(seed).random_raw(4)
        let cases: [(u64, [u64; 4]); 4] = [
            (
                0,
                [
                    0xa30febcfd9c2825f,
                    0x4510bdf882d9d721,
                    0x0a7d3da94ecde8b8,
                    0x043b27b61342f01d,
                ],
            ),
            (
                1,
                [
                    0x8306bdf37922e4ff,
                    0xf35196bbc152a866,
                    0x24e7a4f608ec18cd,
                    0xf2dab0aed2ac6fd2,
                ],
            ),
            (
                42,
                [
                    0xc621fbcd16d92688,
                    0x705a5661a791ffc1,
                    0xdbcd12c26eda1624,
                    0xb286b60e1600888d,
                ],
            ),
            (
                12345,
                [
                    0x3a32b18db2ffc19d,
                    0x51171315c9e4c4de,
                    0xcc2024823444efd9,
                    0xad1f06aea486e910,
                ],
            ),
        ];
        for (seed, want) in cases {
            let mut r = Pcg64::new(seed);
            let got: Vec<u64> = (0..4).map(|_| r.next_u64()).collect();
            assert_eq!(got, want.to_vec(), "seed={seed}");
        }
    }

    #[test]
    fn random_f64_matches_numpy() {
        // numpy: default_rng(seed).random()
        let cases: [(u64, [f64; 3]); 3] = [
            (0, [0.6369616873214543, 0.2697867137638703, 0.04097352393619469]),
            (1, [0.5118216247002567, 0.9504636963259353, 0.14415961271963373]),
            (42, [0.7739560485559633, 0.4388784397520523, 0.8585979199113825]),
        ];
        for (seed, want) in cases {
            let mut r = Pcg64::new(seed);
            for (i, w) in want.iter().enumerate() {
                let g = r.next_f64();
                assert!((g - w).abs() < 1e-15, "seed={seed} i={i}: {g} != {w}");
            }
        }
    }

    #[test]
    fn uniform_matches_numpy() {
        // numpy: default_rng(seed).uniform(-1, 1, 4)
        let want0 = [
            0.2739233746429086,
            -0.4604265724722594,
            -0.9180529521276106,
            -0.9669447289429418,
        ];
        let got = Pcg64::new(0).uniform(-1.0, 1.0, 4);
        for (g, w) in got.iter().zip(&want0) {
            assert!((g - w).abs() < 1e-15, "{g} != {w}");
        }
        let want7 = [
            0.25019093320933394,
            0.794427601939151,
            0.551371380490387,
            -0.5495856200188163,
        ];
        let got = Pcg64::new(7).uniform(-1.0, 1.0, 4);
        for (g, w) in got.iter().zip(&want7) {
            assert!((g - w).abs() < 1e-15, "{g} != {w}");
        }
    }
}
