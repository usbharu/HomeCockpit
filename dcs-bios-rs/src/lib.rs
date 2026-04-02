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

impl<'a, M: MemoryMap + 'a, F: Fn(RangeInclusive<u16>, &'a M)> Listener<'a, M, F> {
    fn contains(&self, range: &RangeInclusive<u16>) -> bool {
        self.address.start() <= range.start() && range.end() <= self.address.end()
    }
}

pub struct DcsBiosImpl<S: Source, M: MemoryMap> {
    source: S,
    memory_map: M,
}

impl<S: Source, M: MemoryMap> DcsBiosImpl<S, M> {
    pub fn new(source: S, memory_map: M) -> Self {
        Self { source, memory_map }
    }

    fn apply_packet(memory_map: &mut M, packet: DcsBiosPacket<'_>) -> Result<(), Error> {
        for write in packet {
            memory_map.write(write.address, write.data)?;
        }

        Ok(())
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
        let Some(bytes) = self.source.read()? else {
            return Ok(());
        };
        let packet = DcsBiosPacket::new(bytes);

        Self::apply_packet(&mut self.memory_map, packet)?;

        for write in packet {
            let range = write.address..=(write.address + (write.length - 1));

            if listener.contains(&range) {
                (listener.func)(range, &self.memory_map);
            }
        }

        Ok(())
    }

    fn read_packet(&mut self) -> Result<DcsBiosPacket<'_>, Error> {
        let Some(bytes) = self.source.read()? else {
            return Ok(DcsBiosPacket::default());
        };
        let packet = DcsBiosPacket::new(bytes);

        Self::apply_packet(&mut self.memory_map, packet)?;

        Ok(packet)
    }
}

#[derive(Debug, Clone, Copy)]
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
        DcsBiosPacket {
            data: &[0; 0],
            next_offset: 0,
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

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::RefCell;
    extern crate std;
    use self::std::{string::String, vec::Vec as StdVec};

    struct TestMemoryMap {
        data: [u8; 0x2000],
    }

    impl TestMemoryMap {
        fn new() -> Self {
            Self { data: [0; 0x2000] }
        }
    }

    impl MemoryMap for TestMemoryMap {
        fn write(&mut self, address: u16, data: &[u8]) -> Result<RangeInclusive<u16>, Error> {
            let start = address as usize;
            let end = start + data.len();
            self.data[start..end].copy_from_slice(data);
            Ok(address..=(address + data.len() as u16 - 1))
        }

        fn read(&self, range: RangeInclusive<u16>) -> Option<&[u8]> {
            let start = *range.start() as usize;
            let end = *range.end() as usize + 1;
            self.data.get(start..end)
        }
    }

    struct MockSource {
        frame: Option<&'static [u8]>,
    }

    impl MockSource {
        fn new(frame: &'static [u8]) -> Self {
            Self { frame: Some(frame) }
        }
    }

    impl Source for MockSource {
        fn setup(&self) -> Result<(), Error> {
            Ok(())
        }

        fn read(&mut self) -> Result<Option<&[u8]>, Error> {
            Ok(self.frame.take())
        }
    }

    #[test]
    fn packet_iteration_requires_frame_sync_prefix() {
        let mut packet = DcsBiosPacket::new(&[0x00, 0x10, 0x02, 0x00, 0x34, 0x12]);
        assert!(packet.next().is_none());
    }

    #[test]
    fn packet_iteration_parses_multiple_writes_after_sync() {
        let bytes = [
            0x55, 0x55, 0x55, 0x55,
            0x00, 0x10, 0x04, 0x00, 0x41, 0x2d, 0x31, 0x30,
            0x10, 0x10, 0x02, 0x00, 0x34, 0x12,
        ];

        let mut packet = DcsBiosPacket::new(&bytes);
        let first = packet.next().expect("first write");
        assert_eq!(first.address, 0x1000);
        assert_eq!(first.length, 4);
        assert_eq!(first.data, b"A-10");

        let second = packet.next().expect("second write");
        assert_eq!(second.address, 0x1010);
        assert_eq!(second.length, 2);
        assert_eq!(second.data, &[0x34, 0x12]);

        assert!(packet.next().is_none());
    }

    #[test]
    fn get_integer_decodes_little_endian_word_using_mask_and_shift() {
        let mut memory = TestMemoryMap::new();
        memory.write(0x1000, &[0b1011_0010, 0b0000_0011]).unwrap();

        let value = DcsBiosImpl::<MockSource, TestMemoryMap>::get_integer(
            &memory,
            0x1000,
            0b0000_0011_1111_0000,
            4,
        );

        assert_eq!(value, Some(0b00_111011));
    }

    #[test]
    fn read_applies_all_writes_in_a_frame_before_notifying_listener() {
        static FRAME: [u8; 16] = [
            0x55, 0x55, 0x55, 0x55,
            0x00, 0x10, 0x02, 0x00, b'A', b'-',
            0x02, 0x10, 0x02, 0x00, b'1', b'0',
        ];
        let source = MockSource::new(&FRAME);
        let memory = TestMemoryMap::new();
        let mut bios = DcsBiosImpl::new(source, memory);
        let observed = RefCell::new(StdVec::<String>::new());

        let listener = Listener {
            _phantom: PhantomData,
            address: 0x1000..=0x1003,
            func: |_, memory: &TestMemoryMap| {
                let value = DcsBiosImpl::<MockSource, TestMemoryMap>::get_string(memory, 0x1000, 4)
                    .expect("string to be present");
                observed.borrow_mut().push(String::from(value));
            },
        };

        bios.read(&listener).unwrap();

        assert_eq!(
            observed.borrow().as_slice(),
            &[String::from("A-10"), String::from("A-10")]
        );
    }
}
