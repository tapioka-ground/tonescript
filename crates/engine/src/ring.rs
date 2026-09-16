//! 送り手1人・受け手1人の受け渡し。
//!
//! なぜ自前か
//! ----------
//! 音を出す側のスレッドでは、**待ってはいけない**し**確保してもいけない**。
//! 締め切り（数ミリ秒）を一度でも落とすとプツッと鳴る。鍵を取る作りだと、
//! 反対側が持っている間そこで止まってしまう。
//!
//! やっていることは「輪にした箱に順番に置いて、順番に取る」だけ。
//! 置き場はあらかじめ全部作っておくので、受け渡しで確保が起きない。
//!
//! 片付けも同じ形で返す
//! --------------------
//! 鳴り終わった音を音側で捨てると、**捨てる（メモリを返す）のに時間が
//! 掛かって**同じことが起きる。なので鳴り終わったものは逆向きの輪へ
//! 載せて返し、捨てるのは向こう側でやる。

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

struct Shared<T> {
    slots: Box<[UnsafeCell<Option<T>>]>,
    mask: usize,
    /// 取った数
    head: AtomicUsize,
    /// 置いた数
    tail: AtomicUsize,
}

// 送り手と受け手が1人ずつであることは型で守る（Tx / Rx は複製できない）。
unsafe impl<T: Send> Send for Shared<T> {}
unsafe impl<T: Send> Sync for Shared<T> {}

/// 置く側。
pub struct Tx<T>(Arc<Shared<T>>);
/// 取る側。
pub struct Rx<T>(Arc<Shared<T>>);

/// `cap` 個ぶんの輪を作る。`cap` は 2 のべき乗へ切り上げる。
pub fn ring<T>(cap: usize) -> (Tx<T>, Rx<T>) {
    let cap = cap.max(2).next_power_of_two();
    let mut slots = Vec::with_capacity(cap);
    for _ in 0..cap {
        slots.push(UnsafeCell::new(None));
    }
    let s = Arc::new(Shared {
        slots: slots.into_boxed_slice(),
        mask: cap - 1,
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    (Tx(s.clone()), Rx(s))
}

impl<T> Tx<T> {
    /// 置く。いっぱいなら**置かずに返す**。捨てない。
    pub fn push(&self, v: T) -> Result<(), T> {
        let tail = self.0.tail.load(Ordering::Relaxed);
        let head = self.0.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) > self.0.mask {
            return Err(v);
        }
        let slot = &self.0.slots[tail & self.0.mask];
        // 受け手はこの位置を tail が進むまで見ない
        unsafe { *slot.get() = Some(v) };
        self.0.tail.store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// 今いくつ入っているか。
    pub fn len(&self) -> usize {
        self.0.tail.load(Ordering::Acquire).wrapping_sub(self.0.head.load(Ordering::Acquire))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<T> Rx<T> {
    /// 取る。空なら `None`。**待たない。**
    pub fn pop(&self) -> Option<T> {
        let head = self.0.head.load(Ordering::Relaxed);
        let tail = self.0.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let slot = &self.0.slots[head & self.0.mask];
        let v = unsafe { (*slot.get()).take() };
        self.0.head.store(head.wrapping_add(1), Ordering::Release);
        v
    }

    pub fn len(&self) -> usize {
        self.0.tail.load(Ordering::Acquire).wrapping_sub(self.0.head.load(Ordering::Acquire))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_and_out_keep_their_order() {
        let (tx, rx) = ring::<u32>(8);
        assert!(rx.pop().is_none(), "空なのに取れた");
        for i in 0..5 {
            tx.push(i).unwrap();
        }
        assert_eq!(rx.len(), 5);
        for i in 0..5 {
            assert_eq!(rx.pop(), Some(i));
        }
        assert!(rx.pop().is_none());
    }

    #[test]
    fn full_gives_the_item_back() {
        let (tx, rx) = ring::<u32>(4);
        for i in 0..4 {
            tx.push(i).unwrap();
        }
        // いっぱい。捨てずに返ってくる
        assert_eq!(tx.push(99), Err(99));
        assert_eq!(rx.pop(), Some(0));
        // 1つ空いたので入る
        assert_eq!(tx.push(99), Ok(()));
        assert_eq!(rx.pop(), Some(1));
    }

    #[test]
    fn it_wraps_around_forever() {
        let (tx, rx) = ring::<usize>(4);
        // 輪の長さの何倍も回す
        for i in 0..1000 {
            tx.push(i).unwrap();
            assert_eq!(rx.pop(), Some(i));
        }
        assert!(rx.is_empty());
    }

    #[test]
    fn two_threads_lose_nothing() {
        let (tx, rx) = ring::<usize>(16);
        const N: usize = 50_000;
        let t = std::thread::spawn(move || {
            let mut i = 0;
            while i < N {
                if tx.push(i).is_ok() {
                    i += 1;
                } else {
                    std::thread::yield_now();
                }
            }
        });
        let mut got = 0;
        while got < N {
            match rx.pop() {
                // 順番も番号も、1つも狂わないこと
                Some(v) => {
                    assert_eq!(v, got, "順番が狂った");
                    got += 1;
                }
                None => std::thread::yield_now(),
            }
        }
        t.join().unwrap();
    }

    #[test]
    fn what_goes_in_comes_out_whole() {
        // 中身が Vec でも（確保を持つものでも）壊れないこと
        let (tx, rx) = ring::<Vec<f32>>(4);
        tx.push(vec![1.0, 2.0, 3.0]).unwrap();
        assert_eq!(rx.pop(), Some(vec![1.0, 2.0, 3.0]));
    }
}
