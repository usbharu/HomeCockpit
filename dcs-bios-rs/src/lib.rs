use std::{marker::PhantomData, ops::RangeInclusive, str, vec::Vec};

use error::Error;
use mem::MemoryMap;
use source::Source;

pub mod error;
pub mod import;
pub mod mem;
pub mod source;

pub trait DcsBios<M: MemoryMap> {
    fn get_self_integer(&self, address: u16, mask: u16, shift_by: u16) -> Option<u16>;
    fn get_self_string(&self, address: u16, length: u16) -> Option<&str>;
    fn read<'a, F: Fn(RangeInclusive<u16>, &'a M)>(
        &'a mut self,
        listener: &Listener<'a, M, F>,
    ) -> Result<(), Error>;
    fn read_packet(&mut self) -> Result<DcsBiosPacket, Error>;

    fn get_integer(memory_map: &M, address: u16, mask: u16, shift_by: u16) -> Option<u16> {
        let data = memory_map.read(address..=(address + 1))?;
        Some((u16::from_le_bytes([data[0], data[1]]) & mask) >> shift_by)
    }

    fn get_string(memory_map: &M, address: u16, length: u16) -> Option<&str> {
        let data = memory_map.read(address..=(address + (length - 1)))?;
        str::from_utf8(data).ok().or(Some("&E&"))
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

    fn apply_packet(memory_map: &mut M, packet: &DcsBiosPacket) -> Result<(), Error> {
        for write in packet.iter() {
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

        Self::apply_packet(&mut self.memory_map, &packet)?;

        for write in packet.iter() {
            let range = write.address..=(write.address + (write.length - 1));

            if listener.contains(&range) {
                (listener.func)(range, &self.memory_map);
            }
        }

        Ok(())
    }

    fn read_packet(&mut self) -> Result<DcsBiosPacket, Error> {
        let Some(bytes) = self.source.read()? else {
            return Ok(DcsBiosPacket::default());
        };
        let packet = DcsBiosPacket::new(bytes);

        Self::apply_packet(&mut self.memory_map, &packet)?;

        Ok(packet)
    }
}

#[derive(Debug, Clone, Default)]
pub struct DcsBiosPacket {
    data: Vec<u8>,
}

impl DcsBiosPacket {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }

    pub fn iter(&self) -> DcsBiosPacketIter<'_> {
        DcsBiosPacketIter {
            data: &self.data,
            next_offset: 0,
        }
    }
}

pub struct DcsBiosPacketIter<'a> {
    data: &'a [u8],
    next_offset: usize,
}

#[derive(Debug)]
pub struct Receive<'a> {
    pub address: u16,
    pub length: u16,
    pub data: &'a [u8],
}

impl<'a> Iterator for DcsBiosPacketIter<'a> {
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

    let address = u16::from_le_bytes(data.get(start..start + 2)?.try_into().ok()?);
    let length = u16::from_le_bytes(data.get(start + 2..start + 4)?.try_into().ok()?);
    let payload = data.get(start + 4..start + 4 + length as usize)?;

    Some((
        Receive {
            address,
            length,
            data: payload,
        },
        start + 4 + length as usize,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, string::String, vec::Vec as StdVec};

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
        frame: Option<Vec<u8>>,
    }

    impl MockSource {
        fn new(frame: &[u8]) -> Self {
            Self {
                frame: Some(frame.to_vec()),
            }
        }
    }

    impl Source for MockSource {
        fn setup(&self) -> Result<(), Error> {
            Ok(())
        }

        fn read(&mut self) -> Result<Option<Vec<u8>>, Error> {
            Ok(self.frame.take())
        }
    }

    #[test]
    fn packet_iteration_requires_frame_sync_prefix() {
        let packet = DcsBiosPacket::new(vec![0x00, 0x10, 0x02, 0x00, 0x34, 0x12]);
        assert!(packet.iter().next().is_none());
    }

    #[test]
    fn packet_iteration_parses_multiple_writes_after_sync() {
        let bytes = vec![
            0x55, 0x55, 0x55, 0x55, 0x00, 0x10, 0x04, 0x00, 0x41, 0x2d, 0x31, 0x30, 0x10, 0x10,
            0x02, 0x00, 0x34, 0x12,
        ];

        let packet = DcsBiosPacket::new(bytes);
        let mut iter = packet.iter();

        let first = iter.next().expect("first write");
        assert_eq!(first.address, 0x1000);
        assert_eq!(first.length, 4);
        assert_eq!(first.data, b"A-10");

        let second = iter.next().expect("second write");
        assert_eq!(second.address, 0x1010);
        assert_eq!(second.length, 2);
        assert_eq!(second.data, &[0x34, 0x12]);

        assert!(iter.next().is_none());
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
        let frame = [
            0x55, 0x55, 0x55, 0x55, 0x00, 0x10, 0x02, 0x00, b'A', b'-', 0x02, 0x10, 0x02, 0x00,
            b'1', b'0',
        ];
        let source = MockSource::new(&frame);
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
