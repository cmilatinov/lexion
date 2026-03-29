use crate::generators::x86::Bitness;

#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub struct Align {
    bits: usize,
}

impl Default for Align {
    fn default() -> Self {
        Self::none()
    }
}

impl Align {
    pub fn none() -> Self {
        Self { bits: 0 }
    }

    pub fn new(bytes: usize) -> Self {
        assert!(bytes.is_power_of_two());
        Self {
            bits: bytes.ilog2() as usize,
        }
    }

    pub fn from_bitness(bitness: Bitness) -> Self {
        Self {
            bits: bitness.bytes().ilog2() as usize,
        }
    }

    pub fn value(&self) -> usize {
        1 << self.bits
    }

    pub fn mask(&self) -> usize {
        self.value() - 1
    }

    pub fn align(&self, offset: usize) -> usize {
        let mask = self.mask();
        (offset + mask) & !mask
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SizeAlign {
    pub size: usize,
    pub align: Align,
}

impl SizeAlign {
    pub fn none() -> Self {
        Self {
            size: 0,
            align: Align::none(),
        }
    }

    pub fn from_size(size: usize) -> Self {
        Self {
            size,
            align: Align::new(size),
        }
    }

    pub fn ptr(bitness: Bitness) -> Self {
        Self {
            size: bitness.bytes(),
            align: Align::from_bitness(bitness),
        }
    }

    pub fn slice(bitness: Bitness) -> Self {
        Self {
            size: 2 * bitness.bytes(),
            align: Align::from_bitness(bitness),
        }
    }
}

pub trait Layout {
    fn size_align(&self) -> SizeAlign;
}

#[derive(Debug, Clone)]
pub struct MemberLayout {
    pub offset: usize,
    pub size_align: SizeAlign,
}

impl Layout for MemberLayout {
    fn size_align(&self) -> SizeAlign {
        self.size_align
    }
}

#[derive(Debug, Clone)]
pub struct MemoryLayout {
    align: Align,
    next: usize,
    members: Vec<MemberLayout>,
}

impl MemoryLayout {
    pub fn incomplete() -> Self {
        Self {
            align: Align::none(),
            next: 0,
            members: vec![],
        }
    }
}

impl Layout for MemoryLayout {
    fn size_align(&self) -> SizeAlign {
        SizeAlign {
            size: self.align.align(self.next),
            align: self.align,
        }
    }
}

pub struct CMemoryLayoutBuilder {
    align: Align,
    next: usize,
    members: Vec<MemberLayout>,
}

pub trait MemoryLayoutBuilder {
    fn new() -> Self;
    fn member(&mut self, size: usize, align: Align) -> usize;
    fn build(self) -> MemoryLayout;
}

impl MemoryLayoutBuilder for CMemoryLayoutBuilder {
    fn new() -> Self {
        Self {
            align: Align::none(),
            next: 0,
            members: Vec::new(),
        }
    }

    fn member(&mut self, size: usize, align: Align) -> usize {
        self.next = align.align(self.next);

        let offset = self.next;
        self.next += size;
        self.align = self.align.max(align);
        self.members.push(MemberLayout {
            offset,
            size_align: SizeAlign { size, align },
        });

        offset
    }

    fn build(self) -> MemoryLayout {
        MemoryLayout {
            align: self.align,
            next: self.next,
            members: self.members,
        }
    }
}
