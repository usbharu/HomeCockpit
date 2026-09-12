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
        let end = start
            .checked_add(data.len())
            .ok_or(Error::MemoryMapError())?;
        let last = end
            .checked_sub(1)
            .and_then(|value| u16::try_from(value).ok())
            .ok_or(Error::MemoryMapError())?;

        if self.map.len() < end {
            self.map.resize(end, 0);
        }

        self.map[start..end].copy_from_slice(data);

        Ok(address..=last)
    }

    fn read(&self, range: RangeInclusive<u16>) -> Option<&[u8]> {
        let start = *range.start() as usize;
        let end = *range.end() as usize + 1;
        self.map.get(start..end)
    }
}

pub type HeaplessMemoryMap = VecMemoryMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_supports_the_last_two_bytes_of_the_address_space() {
        let mut memory = VecMemoryMap::new();

        let range = memory
            .write(0xfffe, &[0x38, 0x03])
            .expect("top-of-space write should be valid");

        assert_eq!(range, 0xfffe..=0xffff);
        assert_eq!(memory.read(0xfffe..=0xffff), Some(&[0x38, 0x03][..]));
    }

    #[test]
    fn write_rejects_data_that_exceeds_the_address_space() {
        let mut memory = VecMemoryMap::new();

        assert!(memory.write(0xffff, &[0x38, 0x03]).is_err());
    }
}
