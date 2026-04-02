#![no_std]

use core::{
    marker::PhantomData,
    ops::RangeInclusive, str,
};

use error::Error;
use mem::MemoryMap;
use source::Source;

pub mod error;
pub mod mem;
pub mod source;

pub trait DcsBios<M: MemoryMap> {
    fn get_self_integer(&self, address: u16, mask: u16, shift_by: u16) -> Option<u16>;
    fn get_self_string(&self, address: u16, length: u16) -> Option<&str>;
    fn read<'a, F: Fn(RangeInclusive<u16>, &'a M)>(
        &'a mut self,
        listener: &Listener<'a, M, F>,
    ) -> Result<(), Error>;

    fn read_packet(&mut self) -> Result<DcsBiosPacket<'_>, Error>;

    fn get_integer(memory_map: &M, address: u16, mask: u16, shift_by: u16) -> Option<u16> {
        let d = memory_map.read(address..=(address + 1))?;
        Some((u16::from_le_bytes([d[0], d[1]]) & mask) >> shift_by)
    }

    fn get_string(memory_map: &M, address: u16, length: u16) -> Option<&str> {
        let d = memory_map.read(address..=(address + (length - 1)))?;
        str::from_utf8(d).ok().or(Some("&E&"))
    }
}

pub struct Listener<'a, M: MemoryMap + 'a, F: Fn(RangeInclusive<u16>, &'a M)> {
    pub _phantom: PhantomData<&'a M>,
    pub address: RangeInclusive<u16>,
    pub func: F,
}

pub struct DcsBiosImpl<S: Source, M: MemoryMap> {
    source: S,
    memory_map: M,
}

impl<S: Source, M: MemoryMap> DcsBiosImpl<S, M> {
    pub fn new(source: S, memory_map: M) -> Self {
        Self { source, memory_map }
    }
}

impl<S: Source, M: MemoryMap> DcsBios<M> for DcsBiosImpl<S, M> {
    fn get_self_integer(&self, address: u16, mask: u16, shift_by: u16) -> Option<u16> {
        DcsBiosImpl::<S, M>::get_integer(&self.memory_map, address, mask, shift_by)
    }

    fn get_self_string(&self, address: u16, length: u16) -> Option<&str> {
        DcsBiosImpl::<S, M>::get_string(&self.memory_map, address, length)
    }

    fn read<'a, F: Fn(RangeInclusive<u16>, &'a M)>(
        &'a mut self,
        listener: &Listener<'a, M, F>,
    ) -> Result<(), Error> {
        let bytes = self.source.read()?;
        if bytes.is_none() {
            return Ok(());
        };
        let bytes = bytes.unwrap();
        let packet = DcsBiosPacket::<'a>::new(bytes);
        for ele in packet {
            let address = ele.address;
            {
                let mem: &mut M = &mut self.memory_map;
                mem.write(address, ele.data)?;
            };
        }
        let packet = DcsBiosPacket::<'a>::new(bytes);
        for ele in packet {
            let address = ele.address;
            let length = ele.length;
            let range = address..=(address + (length - 1));

            if listener.address.start() <= range.start() && range.end() <= listener.address.end()
            {
                (listener.func)(range, &self.memory_map);
            }
        }
        Ok(())
    }

    fn read_packet(&mut self) -> Result<DcsBiosPacket<'_>, Error> {
        let bytes = self.source.read()?;
        if bytes.is_none() {
            return Ok(DcsBiosPacket::default());
        };
        let bytes = bytes.unwrap();
        let packet = DcsBiosPacket::new(bytes);
        for ele in packet {
            let address = ele.address;
            {
                let mem: &mut M = &mut self.memory_map;
                mem.write(address, ele.data)?;
            };
        }
        return Ok(DcsBiosPacket::new(bytes));
    }
}

#[derive(Debug,Clone, Copy)]
pub struct DcsBiosPacket<'a> {
    data: &'a [u8],
    next_offset: usize,
}

impl<'a> DcsBiosPacket<'a> {
    fn new(data: &'a [u8]) -> DcsBiosPacket<'a> {
        DcsBiosPacket {
            data,
            next_offset: 0,
        }
    }
}

impl Default for DcsBiosPacket<'_> {
    fn default() -> Self {
        DcsBiosPacket{
            data: &[0; 0],
            next_offset: 0
        }
    }
}


#[derive(Debug)]
pub struct Receive<'a> {
    pub address: u16,
    pub length: u16,
    pub data: &'a [u8],
}

impl<'a> Iterator for DcsBiosPacket<'a> {
    type Item = Receive<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (receive, offset) = parse_packet_iter(self.data, self.next_offset)?;
        self.next_offset = offset;
        Some(receive)
    }
}

fn parse_packet_iter(data: &[u8], offset: usize) -> Option<(Receive<'_>, usize)> {
    let start = if offset == 0 {
        let sync = data.get(..4)?;
        if sync != [0x55; 4] {
            return None;
        }
        4
    } else {
        offset
    };
    let address = data.get(start..start + 2)?;
    let address = u16::from_le_bytes(match address.try_into() {
        Ok(v) => v,
        Err(_) => return None,
    });
    let len = data.get((start + 2)..(start + 4))?;
    let len: u16 = u16::from_le_bytes(match len.try_into() {
        Ok(v) => v,
        Err(_) => return None,
    });

    let data = data.get((start + 4)..(len as usize + start + 4))?;
    Some((
        Receive {
            address,
            length: len,
            data,
        },
        start + 4 + len as usize,
    ))
}
