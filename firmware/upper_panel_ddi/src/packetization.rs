use imcp::frame::{MAX_ENCODED_FRAME_SIZE, MAX_FRAME_SIZE};

pub const USB_MAX_PACKET_SIZE: usize = 64;
/// The parser must hold a complete encoded IMCP frame plus one transport read.
/// This lets a frame whose EOF arrives in the next 64-byte read be recovered
/// without rejecting the whole read before the parser can inspect it.
pub const IMCP_RX_BUFFER_SIZE: usize = MAX_ENCODED_FRAME_SIZE + USB_MAX_PACKET_SIZE;
pub const IMCP_FRAME_BUFFER_SIZE: usize = MAX_FRAME_SIZE;

/// Keep two application queue slots available for DeviceHello responses.
/// IMCP ACK/Pong responses use a separate priority channel.
pub const FRAME_CHANNEL_CAPACITY: usize = 7;
pub const RESERVED_PROTOCOL_FRAME_SLOTS: usize = 2;

pub const fn can_enqueue_control_event(free_capacity: usize) -> bool {
    free_capacity > RESERVED_PROTOCOL_FRAME_SLOTS
}

/// Keep one complete transport read while the response queue drains. The
/// caller must stop reading the transport until this read has been parsed.
pub struct PendingRead<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> PendingRead<N> {
    pub const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.len != 0
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub fn store(&mut self, bytes: &[u8]) -> bool {
        if self.is_pending() || bytes.len() > N {
            return false;
        }
        self.bytes[..bytes.len()].copy_from_slice(bytes);
        self.len = bytes.len();
        true
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }
}

impl<const N: usize> Default for PendingRead<N> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn next_packet_len(total_len: usize, offset: usize, packet_size: usize) -> usize {
    total_len.saturating_sub(offset).min(packet_size)
}

pub fn needs_zero_length_packet(total_len: usize, packet_size: usize) -> bool {
    packet_size != 0 && total_len != 0 && total_len.is_multiple_of(packet_size)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{
        FRAME_CHANNEL_CAPACITY, IMCP_FRAME_BUFFER_SIZE, IMCP_RX_BUFFER_SIZE, PendingRead,
        RESERVED_PROTOCOL_FRAME_SLOTS, USB_MAX_PACKET_SIZE, can_enqueue_control_event,
        needs_zero_length_packet, next_packet_len,
    };
    use imcp::frame::{MAX_ENCODED_FRAME_SIZE, MAX_FRAME_SIZE};

    fn packet_lengths(total_len: usize) -> std::vec::Vec<usize> {
        let mut offset = 0;
        let mut lengths = std::vec::Vec::new();
        while offset < total_len {
            let length = next_packet_len(total_len, offset, USB_MAX_PACKET_SIZE);
            if length == 0 {
                break;
            }
            lengths.push(length);
            offset += length;
        }
        if needs_zero_length_packet(total_len, USB_MAX_PACKET_SIZE) {
            lengths.push(0);
        }
        lengths
    }

    #[test]
    fn packetization_keeps_short_frames_in_one_packet() {
        assert_eq!(packet_lengths(12), vec![12]);
    }

    #[test]
    fn packetization_emits_zlp_at_exact_boundary() {
        assert_eq!(packet_lengths(USB_MAX_PACKET_SIZE), vec![64, 0]);
    }

    #[test]
    fn packetization_splits_multiple_packets() {
        assert_eq!(packet_lengths(USB_MAX_PACKET_SIZE * 2 + 3), vec![64, 64, 3]);
    }

    #[test]
    fn parser_buffers_cover_the_wire_and_decoded_frame_limits() {
        assert_eq!(IMCP_FRAME_BUFFER_SIZE, MAX_FRAME_SIZE);
        assert_eq!(
            IMCP_RX_BUFFER_SIZE,
            MAX_ENCODED_FRAME_SIZE + USB_MAX_PACKET_SIZE
        );
    }

    #[test]
    fn application_events_leave_protocol_response_capacity() {
        assert!(can_enqueue_control_event(FRAME_CHANNEL_CAPACITY));
        assert!(!can_enqueue_control_event(RESERVED_PROTOCOL_FRAME_SLOTS));
        assert!(!can_enqueue_control_event(
            RESERVED_PROTOCOL_FRAME_SLOTS - 1
        ));
    }

    #[test]
    fn pending_transport_read_preserves_exact_bytes_until_parsed() {
        let wire = [0xfe, 0x01, 0x02, 0x00, 0x00, 0x00, 0x03, 0xff];
        let mut pending = PendingRead::<USB_MAX_PACKET_SIZE>::new();
        assert!(pending.store(&wire));
        assert!(pending.is_pending());
        assert!(!pending.store(&[0xaa]));
        assert_eq!(pending.bytes(), &wire);

        let mut rx_buffer = [0u8; 32];
        let mut frame_buffer = [0u8; 32];
        let mut parser = imcp::parser::FrameParser::new(&mut rx_buffer, &mut frame_buffer);
        parser.write_data(pending.bytes()).unwrap();
        assert!(parser.next_frame().unwrap().is_ok());
        pending.clear();
        assert!(!pending.is_pending());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod properties {
    use super::{
        PendingRead, RESERVED_PROTOCOL_FRAME_SLOTS, USB_MAX_PACKET_SIZE, can_enqueue_control_event,
        needs_zero_length_packet, next_packet_len,
    };
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn packet_lengths_sum_to_the_payload(
            total in 0usize..=512,
            packet_size in 0usize..=USB_MAX_PACKET_SIZE,
        ) {
            if packet_size == 0 {
                prop_assert_eq!(next_packet_len(total, 0, 0), 0);
                prop_assert!(!needs_zero_length_packet(total, 0));
                return Ok(());
            }

            let mut offset = 0;
            let mut sum = 0;
            while offset < total {
                let len = next_packet_len(total, offset, packet_size);
                prop_assert!(len > 0 && len <= packet_size);
                prop_assert!(offset + len <= total);
                offset += len;
                sum += len;
            }
            prop_assert_eq!(sum, total);
            prop_assert_eq!(
                needs_zero_length_packet(total, packet_size),
                total != 0 && total.is_multiple_of(packet_size)
            );
        }

        #[test]
        fn control_events_keep_protocol_slots(free in 0usize..16) {
            prop_assert_eq!(
                can_enqueue_control_event(free),
                free > RESERVED_PROTOCOL_FRAME_SLOTS
            );
        }

        #[test]
        fn pending_read_rejects_a_second_store_until_clear(
            first in prop::collection::vec(any::<u8>(), 0..=8),
            second in prop::collection::vec(any::<u8>(), 0..=12),
        ) {
            const N: usize = 8;
            let mut pending = PendingRead::<N>::new();
            let stored = pending.store(&first);
            if first.len() > N {
                prop_assert!(!stored);
                prop_assert!(!pending.is_pending());
                return Ok(());
            }

            prop_assert!(stored);
            prop_assert_eq!(pending.is_pending(), !first.is_empty());
            prop_assert_eq!(pending.bytes(), first.as_slice());
            if first.is_empty() {
                prop_assert_eq!(pending.store(&second), second.len() <= N);
                return Ok(());
            }

            prop_assert!(!pending.store(&second));
            prop_assert_eq!(pending.bytes(), first.as_slice());
            pending.clear();
            prop_assert!(!pending.is_pending());
            prop_assert_eq!(pending.store(&second), second.len() <= N);
        }
    }
}

#[cfg(kani)]
mod proofs {
    use super::{PendingRead, needs_zero_length_packet, next_packet_len};

    #[kani::proof]
    #[kani::unwind(5)]
    fn packet_lengths_sum_to_the_payload() {
        let total: usize = kani::any();
        kani::assume(total <= 192);
        let packet_size = 64usize;
        let mut offset = 0usize;
        let mut sum = 0usize;
        while offset < total {
            let len = next_packet_len(total, offset, packet_size);
            assert!(len > 0 && len <= packet_size);
            assert!(offset + len <= total);
            offset += len;
            sum += len;
        }
        assert_eq!(sum, total);
        assert_eq!(
            needs_zero_length_packet(total, packet_size),
            total != 0 && total.is_multiple_of(packet_size)
        );
    }

    #[kani::proof]
    fn zero_packet_size_never_requests_a_zero_length_packet() {
        let total: usize = kani::any();
        kani::assume(total <= 192);
        assert!(!needs_zero_length_packet(total, 0));
        assert_eq!(next_packet_len(total, 0, 0), 0);
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn pending_read_stores_exact_bytes_and_rejects_a_second_store() {
        const N: usize = 4;
        let len: usize = kani::any();
        kani::assume(len > 0 && len <= N);
        let data: [u8; N] = kani::any();
        let mut pending = PendingRead::<N>::new();
        assert!(pending.store(&data[..len]));
        assert_eq!(pending.bytes(), &data[..len]);
        assert!(pending.is_pending());

        let again: usize = kani::any();
        kani::assume(again <= N);
        let more: [u8; N] = kani::any();
        assert!(!pending.store(&more[..again]));
        assert_eq!(pending.bytes(), &data[..len]);

        pending.clear();
        assert!(!pending.is_pending());
        assert!(pending.store(&data[..len]));
        assert_eq!(pending.bytes(), &data[..len]);
    }

    #[kani::proof]
    fn pending_read_rejects_bytes_longer_than_capacity() {
        const N: usize = 4;
        let len: usize = kani::any();
        kani::assume(len > N && len <= N * 2);
        let data: [u8; N * 2] = kani::any();
        let mut pending = PendingRead::<N>::new();
        assert!(!pending.store(&data[..len]));
        assert!(!pending.is_pending());
        assert!(pending.bytes().is_empty());
    }
}
