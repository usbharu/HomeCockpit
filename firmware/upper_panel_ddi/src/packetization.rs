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
