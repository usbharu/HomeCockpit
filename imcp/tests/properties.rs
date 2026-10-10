#![cfg(feature = "test-utils")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;

use futures::executor::block_on;
use imcp::{
    EOF, ESC, Imcp, SOF,
    channel::Sender,
    frame::{
        Address, Frame, FramePayload, MAX_ENCODED_FRAME_SIZE, MAX_FRAME_SIZE, MAX_PAYLOAD_SIZE,
    },
    imcp_test::{decode_single_encoded_frame, memory_channel},
    parser::FrameParser,
};
use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;

fn arb_address() -> impl Strategy<Value = Address> {
    prop_oneof![
        Just(Address::Broadcast),
        (0u8..0xFF).prop_map(Address::Unicast),
    ]
}

fn arb_bytes(max_len: usize) -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        3 => prop::collection::vec(any::<u8>(), 0..=max_len),
        1 => prop::collection::vec(prop_oneof![Just(SOF), Just(EOF), Just(ESC)], 0..=max_len),
    ]
}

fn arb_payload() -> impl Strategy<Value = FramePayload> {
    prop_oneof![
        Just(FramePayload::Ping),
        Just(FramePayload::Pong),
        any::<u8>().prop_map(FramePayload::Ack),
        any::<u32>().prop_map(FramePayload::Join),
        (any::<u8>(), any::<u32>())
            .prop_map(|(address, id)| FramePayload::SetAddress { address, id }),
        arb_bytes(MAX_PAYLOAD_SIZE).prop_map(|bytes| {
            FramePayload::Data(heapless::Vec::from_slice(&bytes).expect("payload fits"))
        }),
        arb_bytes(32).prop_map(|bytes| {
            FramePayload::Set(heapless::Vec::from_slice(&bytes).expect("payload fits"))
        }),
    ]
}

fn arb_frame() -> impl Strategy<Value = Frame> {
    (arb_address(), any::<u8>(), arb_payload())
        .prop_map(|(to, from, payload)| Frame::new(to, from, payload))
}

fn encode_frame(frame: &Frame) -> Vec<u8> {
    let mut buffer = vec![0u8; MAX_ENCODED_FRAME_SIZE];
    let length = frame
        .encode(&mut buffer)
        .expect("encoded frame fits in MAX_ENCODED_FRAME_SIZE");
    buffer.truncate(length);
    buffer
}

fn split_points(len: usize, raw: &[usize]) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let mut points: Vec<usize> = raw
        .iter()
        .map(|point| point % len)
        .filter(|point| *point > 0)
        .collect();
    points.sort_unstable();
    points.dedup();
    points.push(len);
    points
}

fn parse_chunks(bytes: &[u8], cuts: &[usize]) -> Vec<Result<Frame, imcp::error::DecodeError>> {
    let mut rx_buffer = vec![0u8; MAX_ENCODED_FRAME_SIZE * 2];
    let mut frame_buffer = vec![0u8; MAX_FRAME_SIZE];
    let mut parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);
    parser
        .write_data(&[])
        .expect("empty write fits in the parser buffer");

    let mut previous = 0;
    for end in cuts {
        parser
            .write_data(&bytes[previous..*end])
            .expect("chunk fits in the parser buffer");
        previous = *end;
    }
    if cuts.is_empty() {
        parser
            .write_data(bytes)
            .expect("frame fits in the parser buffer");
    }

    let mut frames = Vec::new();
    while let Some(frame) = parser.next_frame() {
        frames.push(frame);
    }
    frames
}

fn assignment(frame: &Frame) -> Option<(u8, u32)> {
    match frame.payload() {
        FramePayload::SetAddress { address, id } => Some((*address, *id)),
        _ => None,
    }
}

proptest! {
    #[test]
    fn encoded_frames_roundtrip_through_arbitrary_chunks(
        frame in arb_frame(),
        raw_cuts in prop::collection::vec(any::<usize>(), 0..8),
    ) {
        let encoded = encode_frame(&frame);
        prop_assert_eq!(encoded.len(), frame.encoded_len());
        let frames = parse_chunks(&encoded, &split_points(encoded.len(), &raw_cuts));
        prop_assert_eq!(frames, vec![Ok(frame)]);
    }

    #[test]
    fn parser_resyncs_after_noise_and_a_broken_frame(
        frame in arb_frame(),
        noise_len in 0usize..24,
    ) {
        let encoded = encode_frame(&frame);
        let mut stream = vec![0x11u8; noise_len];
        // Header says the payload is 1 byte, but the body ends at the checksum.
        // The checksum matches, and none of the body bytes are SOF, EOF, or ESC.
        stream.extend_from_slice(&[SOF, 0x01, 0x02, 0x00, 0x01, 0x00, 0x02, EOF]);
        stream.extend_from_slice(&encoded);

        let frames = parse_chunks(&stream, &[stream.len()]);
        prop_assert_eq!(
            frames,
            vec![
                Err(imcp::error::DecodeError::InvalidPayloadLength),
                Ok(frame),
            ]
        );
    }

    #[test]
    fn a_second_write_still_parses_after_the_first_frame_is_consumed(
        first in arb_frame(),
        second in arb_frame(),
    ) {
        let first_bytes = encode_frame(&first);
        let second_bytes = encode_frame(&second);
        let mut rx_buffer = vec![0u8; MAX_ENCODED_FRAME_SIZE];
        let mut frame_buffer = vec![0u8; MAX_FRAME_SIZE];
        let mut parser = FrameParser::new(&mut rx_buffer, &mut frame_buffer);

        parser
            .write_data(&first_bytes)
            .expect("first frame fits");
        prop_assert_eq!(parser.next_frame(), Some(Ok(first)));
        parser.write_data(&[]).expect("empty write fits");
        parser
            .write_data(&second_bytes)
            .expect("second frame fits");
        prop_assert_eq!(parser.next_frame(), Some(Ok(second)));
        prop_assert!(parser.next_frame().is_none());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn assignment_addresses_stay_unique_when_retries_are_dropped(
        ids in prop::collection::vec(any::<u32>(), 1..5),
        extra_retries in prop::collection::vec(0u8..3, 4),
    ) {
        block_on(async {
            let (master_tx, master_rx) = memory_channel();
            let (client_tx, client_rx) = memory_channel();
            let mut master_rx_buf = [0u8; 256];
            let mut master_frame_buf = [0u8; 256];
            let mut client_rx_buf = [0u8; 256];
            let mut client_frame_buf = [0u8; 256];
            let mut master = Imcp::new_master(
                master_rx,
                master_tx,
                &mut master_rx_buf,
                &mut master_frame_buf,
            );
            let mut client = Imcp::new_client(
                client_rx,
                client_tx,
                &mut client_rx_buf,
                &mut client_frame_buf,
            );

            let mut expected_address = 0x02u8;
            let mut seen = HashSet::new();
            for (index, id) in ids.iter().copied().enumerate() {
                client.send_join(id).await.unwrap();
                let join_bytes = client.write_tick().await.unwrap();
                let joined = master.read_tick(&join_bytes).await.unwrap().unwrap();
                prop_assert_eq!(joined.payload(), &FramePayload::Join(id));

                let mut set_bytes = master.write_tick().await.unwrap();
                let extra = extra_retries[index % extra_retries.len()];
                for _ in 0..extra {
                    set_bytes = master.write_tick().await.unwrap();
                }
                let set_frame = decode_single_encoded_frame(&set_bytes).unwrap();
                prop_assert_eq!(assignment(&set_frame), Some((expected_address, id)));

                client.read_tick(&set_bytes).await.unwrap().unwrap();
                let ack_bytes = client.write_tick().await.unwrap();
                master.read_tick(&ack_bytes).await.unwrap().unwrap();
                prop_assert_eq!(client.address(), expected_address);
                prop_assert!(seen.insert(expected_address));
                expected_address = expected_address.saturating_add(1);
            }
            Ok(())
        })?;
    }

    #[test]
    fn a_different_join_does_not_allocate_while_assignment_is_pending(
        first in any::<u32>(),
        second in any::<u32>(),
    ) {
        prop_assume!(first != second);
        block_on(async {
            let (master_tx, master_rx) = memory_channel();
            let (client_tx, client_rx) = memory_channel();
            let mut master_rx_buf = [0u8; 256];
            let mut master_frame_buf = [0u8; 256];
            let mut client_rx_buf = [0u8; 256];
            let mut client_frame_buf = [0u8; 256];
            let mut master = Imcp::new_master(
                master_rx,
                master_tx,
                &mut master_rx_buf,
                &mut master_frame_buf,
            );
            let mut client = Imcp::new_client(
                client_rx,
                client_tx,
                &mut client_rx_buf,
                &mut client_frame_buf,
            );

            client.send_join(first).await.unwrap();
            let join_bytes = client.write_tick().await.unwrap();
            master.read_tick(&join_bytes).await.unwrap().unwrap();
            let first_set = decode_single_encoded_frame(&master.write_tick().await.unwrap()).unwrap();
            prop_assert_eq!(assignment(&first_set), Some((0x02, first)));

            // The client's own write queue would retry the first Join. Feed the
            // second Join on the wire so only the master's assignment rule is tested.
            let other_join = encode_frame(&Frame::new(
                Address::Unicast(0x01),
                0x00,
                FramePayload::Join(second),
            ));
            prop_assert!(master.read_tick(&other_join).await.unwrap().is_none());
            let retried = decode_single_encoded_frame(&master.write_tick().await.unwrap()).unwrap();
            prop_assert_eq!(assignment(&retried), Some((0x02, first)));
            Ok(())
        })?;
    }
}

#[test]
fn abandon_pending_set_clears_only_an_unacknowledged_set() {
    block_on(async {
        let (tx, rx) = memory_channel();
        let mut injector = tx.clone();
        let mut rx_buf = [0u8; 256];
        let mut frame_buf = [0u8; 256];
        let mut master = Imcp::new_master(rx, tx, &mut rx_buf, &mut frame_buf);

        injector
            .send(Frame::new(
                Address::Unicast(0x01),
                0x22,
                FramePayload::Set(heapless::Vec::from_slice(&[0x10, 0x20]).unwrap()),
            ))
            .await
            .unwrap();
        master.write_tick().await.unwrap();
        assert!(master.abandon_pending_set());
        assert!(!master.abandon_pending_set());

        injector
            .send(Frame::new(Address::Broadcast, 0x22, FramePayload::Ping))
            .await
            .unwrap();
        master.write_tick().await.unwrap();
        assert!(!master.abandon_pending_set());
    });
}
