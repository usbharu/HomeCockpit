use imcp::frame::{MAX_ENCODED_FRAME_SIZE, MAX_FRAME_SIZE};

pub const USB_MAX_PACKET_SIZE: usize = 64;
/// The parser must hold a complete encoded IMCP frame plus one transport read.
/// This lets a frame whose EOF arrives in the next 64-byte read be recovered
/// without rejecting the whole read before the parser can inspect it.
pub const IMCP_RX_BUFFER_SIZE: usize = MAX_ENCODED_FRAME_SIZE + USB_MAX_PACKET_SIZE;
pub const IMCP_FRAME_BUFFER_SIZE: usize = MAX_FRAME_SIZE;

/// Two slots are reserved for protocol responses generated while processing a
/// single incoming frame (ACK and DeviceHello).
pub const FRAME_CHANNEL_CAPACITY: usize = 7;
pub const RESERVED_PROTOCOL_FRAME_SLOTS: usize = 2;

pub const fn can_enqueue_control_event(free_capacity: usize) -> bool {
    free_capacity > RESERVED_PROTOCOL_FRAME_SLOTS
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
        FRAME_CHANNEL_CAPACITY, IMCP_FRAME_BUFFER_SIZE, IMCP_RX_BUFFER_SIZE,
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
}
