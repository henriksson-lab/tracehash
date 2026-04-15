const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Clone, Copy)]
pub struct Fnv64 {
    state: u64,
}

impl Fnv64 {
    #[inline]
    pub fn new() -> Self {
        Self { state: FNV_OFFSET }
    }

    #[inline]
    pub fn u8(&mut self, value: u8) {
        self.state ^= value as u64;
        self.state = self.state.wrapping_mul(FNV_PRIME);
    }

    #[inline]
    pub fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    #[inline]
    pub fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    #[inline]
    pub fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.u8(byte);
        }
    }

    #[inline]
    pub fn str(&mut self, value: &str) {
        self.u64(value.len() as u64);
        self.bytes(value.as_bytes());
    }

    #[inline]
    pub fn finish(self) -> u64 {
        self.state
    }
}

impl Default for Fnv64 {
    fn default() -> Self {
        Self::new()
    }
}
