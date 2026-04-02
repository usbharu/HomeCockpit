use core::ops::RangeInclusive;

use heapless::Vec;

use crate::error::Error;

pub trait MemoryMap {
    fn write(&mut self, address: u16, data: &[u8]) -> Result<RangeInclusive<u16>, Error>;
    fn read(&self, range: RangeInclusive<u16>) -> Option<&[u8]>;
}

pub struct HeaplessMemoryMap {
    map: Vec<u8, 65535>,
}

impl MemoryMap for HeaplessMemoryMap {
    fn write(&mut self, address: u16, data: &[u8]) -> Result<RangeInclusive<u16>, Error> {
        for (index, ele) in data.iter().enumerate() {
            self.map
                .insert(address as usize + index, *ele)
                .map_err(|_| Error::MemoryMapError())?;
        }
        Ok(address..=(address + data.len() as u16 - 1))
    }

    fn read(&self, range: RangeInclusive<u16>) -> Option<&[u8]> {
        let range = (*range.start() as usize)..=(*range.end() as usize);
        self.map.get(range)
    }
}
