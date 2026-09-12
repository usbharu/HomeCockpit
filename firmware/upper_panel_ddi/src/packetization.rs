pub const USB_MAX_PACKET_SIZE: usize = 64;

pub(crate) fn next_packet_len(total_len: usize, offset: usize, packet_size: usize) -> usize {
    total_len.saturating_sub(offset).min(packet_size)
}

pub(crate) fn needs_zero_length_packet(total_len: usize, packet_size: usize) -> bool {
    packet_size != 0 && total_len != 0 && total_len % packet_size == 0
}

#[cfg(test)]
mod tests {
    use super::{USB_MAX_PACKET_SIZE, needs_zero_length_packet, next_packet_len};

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
}
