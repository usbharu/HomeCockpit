use std::{ops::RangeInclusive, vec::Vec};

use crate::error::Error;

pub trait MemoryMap {
    fn write(&mut self, address: u16, data: &[u8]) -> Result<RangeInclusive<u16>, Error>;
    fn read(&self, range: RangeInclusive<u16>) -> Option<&[u8]>;
}

pub struct VecMemoryMap {
    map: Vec<u8>,
}

impl VecMemoryMap {
    pub fn new() -> Self {
        Self { map: Vec::new() }
    }
}

impl Default for VecMemoryMap {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryMap for VecMemoryMap {
    fn write(&mut self, address: u16, data: &[u8]) -> Result<RangeInclusive<u16>, Error> {
        let start = address as usize;
        let end = start + data.len();

        if self.map.len() < end {
            self.map.resize(end, 0);
        }

        self.map[start..end].copy_from_slice(data);

        Ok(address..=(address + data.len() as u16 - 1))
    }

    fn read(&self, range: RangeInclusive<u16>) -> Option<&[u8]> {
        let start = *range.start() as usize;
        let end = *range.end() as usize + 1;
        self.map.get(start..end)
    }
}

pub type HeaplessMemoryMap = VecMemoryMap;
