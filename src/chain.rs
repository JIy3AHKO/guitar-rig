//! The signal chain model: which modules are connected, and in which order.
//!
//! The chain is shared between the GUI thread (which edits it) and the audio
//! thread (which reads it every block). To stay real-time safe it is packed into
//! a single `AtomicU64`: 4 bits per slot, up to 16 slots, `0` means "empty".

use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_SLOTS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    Tuner,
    Distortion,
    Amp,
    Delay,
    Reverb,
}

impl ModuleKind {
    pub const ALL: [ModuleKind; 5] = [
        ModuleKind::Tuner,
        ModuleKind::Distortion,
        ModuleKind::Amp,
        ModuleKind::Delay,
        ModuleKind::Reverb,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ModuleKind::Tuner => "Tuner",
            ModuleKind::Distortion => "Distortion",
            ModuleKind::Amp => "Amp",
            ModuleKind::Delay => "Delay",
            ModuleKind::Reverb => "Reverb",
        }
    }

    fn code(self) -> u64 {
        self as u64 + 1
    }

    fn from_code(code: u64) -> Option<Self> {
        Self::ALL.get((code as usize).checked_sub(1)?).copied()
    }
}

/// A decoded, fixed-capacity chain (no heap allocation, usable on the audio thread).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Chain {
    slots: [Option<ModuleKind>; MAX_SLOTS],
    len: usize,
}

impl Chain {
    pub fn from_slice(kinds: &[ModuleKind]) -> Self {
        let mut chain = Chain::default();
        for &k in kinds.iter().take(MAX_SLOTS) {
            chain.slots[chain.len] = Some(k);
            chain.len += 1;
        }
        chain
    }

    pub fn default_chain() -> Self {
        Self::from_slice(&ModuleKind::ALL)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = ModuleKind> + '_ {
        self.slots[..self.len].iter().filter_map(|s| *s)
    }

    pub fn contains(&self, kind: ModuleKind) -> bool {
        self.iter().any(|k| k == kind)
    }

    pub fn position(&self, kind: ModuleKind) -> Option<usize> {
        self.iter().position(|k| k == kind)
    }

    pub fn to_vec(&self) -> Vec<ModuleKind> {
        self.iter().collect()
    }

    fn pack(&self) -> u64 {
        self.iter()
            .enumerate()
            .fold(0u64, |acc, (i, k)| acc | (k.code() << (i * 4)))
    }

    fn unpack(packed: u64) -> Self {
        let mut chain = Chain::default();
        for i in 0..MAX_SLOTS {
            if let Some(k) = ModuleKind::from_code((packed >> (i * 4)) & 0xF) {
                // Each module type appears at most once in this version.
                if !chain.contains(k) {
                    chain.slots[chain.len] = Some(k);
                    chain.len += 1;
                }
            }
        }
        chain
    }
}

/// Wrapper used by the plugin. The inner atomic is what gets persisted with the
/// plugin state (see `GuitarRigParams::chain`).
pub fn new_shared_chain() -> AtomicU64 {
    AtomicU64::new(Chain::default_chain().pack())
}

pub fn load(shared: &AtomicU64) -> Chain {
    Chain::unpack(shared.load(Ordering::Acquire))
}

pub fn store(shared: &AtomicU64, chain: &Chain) {
    shared.store(chain.pack(), Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_roundtrip() {
        let c = Chain::from_slice(&[ModuleKind::Reverb, ModuleKind::Tuner, ModuleKind::Amp]);
        assert_eq!(Chain::unpack(c.pack()), c);
        assert_eq!(Chain::unpack(0).len(), 0);
        assert_eq!(Chain::unpack(Chain::default_chain().pack()).len(), 5);
    }
}
