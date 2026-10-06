//! `lrand48` as used by the original (`CGachaManager::GetRandomPrize @0010d5dc..` calls `lrand48()` for the weighted pick).
//! 48 bit LCG: X(n+1) = (0x5DEECE66D * X(n) + 0xB) mod 2^48, result = X >> 17 (31 bits). `srand48(s)`: X = (s << 16) | 0x330E.

#[derive(Clone, Debug)]
pub struct Lrand48 {
    x: u64,
}

impl Lrand48 {
    pub fn new(seed: u32) -> Lrand48 {
        Lrand48 { x: ((seed as u64) << 16) | 0x330E }
    }
    /// Seed from the system clock (offline: there is no server time).
    pub fn from_clock() -> Lrand48 {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        Lrand48::new((n ^ (n >> 32)) as u32)
    }
    /// `lrand48()`: 0 .. 2^31-1
    pub fn next(&mut self) -> u32 {
        self.x = (self.x.wrapping_mul(0x5DEECE66D).wrapping_add(0xB)) & 0xFFFF_FFFF_FFFF;
        (self.x >> 17) as u32
    }
    /// uniform float in [0, 1)
    pub fn unit(&mut self) -> f32 {
        (self.next() as f64 / 2147483648.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lrand48_matches_libc() {
        // glibc: srand48(1); lrand48() -> 89400484, 976015093, 1792756325
        let mut r = Lrand48::new(1);
        assert_eq!(r.next(), 89400484);
        assert_eq!(r.next(), 976015093);
        assert_eq!(r.next(), 1792756325);
    }
}
