#![cfg(feature = "test-utils")]

use futures::executor::block_on;
use imcp::{
    error::{ImcpError, ProtocolError},
    Imcp,
    channel::Sender,
    frame::{Address, Frame, FramePayload},
    imcp_test::{decode_single_encoded_frame, memory_channel},
};

struct Harness {
    master: Imcp<
        'static,
        'static,
        imcp::imcp_test::MemoryReceiver,
        imcp::imcp_test::MemorySender,
    >,
    client: Imcp<
        'static,
        'static,
        imcp::imcp_test::MemoryReceiver,
        imcp::imcp_test::MemorySender,
    >,
    master_injector: imcp::imcp_test::MemorySender,
}

fn new_harness() -> Harness {
    let (master_tx_sender, master_tx_receiver) = memory_channel();
    let master_injector = master_tx_sender.clone();
    let (client_tx_sender, client_tx_receiver) = memory_channel();
    let master_rx_buf = Box::leak(Box::new([0u8; 128]));
    let master_frame_buf = Box::leak(Box::new([0u8; 128]));
    let client_rx_buf = Box::leak(Box::new([0u8; 128]));
    let client_frame_buf = Box::leak(Box::new([0u8; 128]));

    let master = Imcp::new_master(
        master_tx_receiver,
        master_tx_sender,
        master_rx_buf,
        master_frame_buf,
    );
    let client = Imcp::new_client(
        client_tx_receiver,
        client_tx_sender,
        client_rx_buf,
        client_frame_buf,
    );

    Harness {
        master,
        client,
        master_injector,
    }
}

async fn join_client(harness: &mut Harness, id: u32) {
    harness.client.send_join(id).await.unwrap();
    let join_bytes = harness.client.write_tick().await.unwrap();
    harness.master.read_tick(&join_bytes).await.unwrap();
    let set_address_bytes = harness.master.write_tick().await.unwrap();
    harness.client.read_tick(&set_address_bytes).await.unwrap();
    let ack_bytes = harness.client.write_tick().await.unwrap();
    harness.master.read_tick(&ack_bytes).await.unwrap();
}

#[test]
fn join_and_set_address_roundtrip_works_on_os() {
    block_on(async {
        let mut harness = new_harness();

        harness.client.send_join(0xCAFE_BABE).await.unwrap();
        let join_bytes = harness.client.write_tick().await.unwrap();
        let join = decode_single_encoded_frame(&join_bytes).unwrap();
        assert_eq!(join.payload(), &FramePayload::Join(0xCAFE_BABE));

        let master_seen = harness.master.read_tick(&join_bytes).await.unwrap().unwrap();
        assert_eq!(master_seen.payload(), &FramePayload::Join(0xCAFE_BABE));

        let set_address_bytes = harness.master.write_tick().await.unwrap();
        let set_address = decode_single_encoded_frame(&set_address_bytes).unwrap();
        assert!(matches!(
            set_address.payload(),
            FramePayload::SetAddress {
                address: 0x02,
                id: 0xCAFE_BABE
            }
        ));

        let client_seen = harness
            .client
            .read_tick(&set_address_bytes)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            client_seen.payload(),
            FramePayload::SetAddress {
                address: 0x02,
                id: 0xCAFE_BABE
            }
        ));

        let ack_bytes = harness.client.write_tick().await.unwrap();
        let ack = decode_single_encoded_frame(&ack_bytes).unwrap();
        assert_eq!(ack.payload(), &FramePayload::Ack(0x00));

        let master_ack = harness.master.read_tick(&ack_bytes).await.unwrap().unwrap();
        assert_eq!(master_ack.payload(), &FramePayload::Ack(0x00));
    });
}

#[test]
fn set_frame_gets_ack_on_os() {
    block_on(async {
        let mut harness = new_harness();

        join_client(&mut harness, 0xDEAD_BEEF).await;

        harness
            .master_injector
            .send(Frame::new(
                Address::Unicast(0x02),
                0x01,
                FramePayload::Set(heapless::Vec::from_slice(&[0x10, 0x20]).unwrap()),
            ))
            .await
            .unwrap();

        let set_bytes = harness.master.write_tick().await.unwrap();
        let seen = harness.client.read_tick(&set_bytes).await.unwrap().unwrap();
        assert_eq!(
            seen.payload(),
            &FramePayload::Set(heapless::Vec::from_slice(&[0x10, 0x20]).unwrap())
        );

        let ack_bytes = harness.client.write_tick().await.unwrap();
        let ack = decode_single_encoded_frame(&ack_bytes).unwrap();
        assert_eq!(ack.payload(), &FramePayload::Ack(0x02));
    });
}

#[test]
fn ping_gets_pong_on_os() {
    block_on(async {
        let mut harness = new_harness();

        join_client(&mut harness, 0xFACE_CAFE).await;

        harness
            .master_injector
            .send(Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ping))
            .await
            .unwrap();

        let ping_bytes = harness.master.write_tick().await.unwrap();
        let seen = harness.client.read_tick(&ping_bytes).await.unwrap().unwrap();
        assert_eq!(seen.payload(), &FramePayload::Ping);

        let pong_bytes = harness.client.write_tick().await.unwrap();
        let pong = decode_single_encoded_frame(&pong_bytes).unwrap();
        assert_eq!(pong.payload(), &FramePayload::Pong);
        assert_eq!(pong.from_address(), 0x02);
        assert_eq!(pong.to_address(), Address::Unicast(0x01));
    });
}

#[test]
fn unrelated_unicast_is_ignored_on_os() {
    block_on(async {
        let mut harness = new_harness();

        let frame = Frame::new(Address::Unicast(0x77), 0x01, FramePayload::Ping);
        let mut raw = [0u8; 32];
        let len = frame.encode(&mut raw).unwrap();

        let seen = harness.client.read_tick(&raw[..len]).await.unwrap();

        assert!(seen.is_none());
    });
}

#[test]
fn duplicate_set_address_is_reacked_on_os() {
    block_on(async {
        let mut harness = new_harness();

        join_client(&mut harness, 0x1234_ABCD).await;

        let duplicate_set_address = Frame::new(
            Address::Unicast(0x02),
            0x01,
            FramePayload::SetAddress {
                address: 0x02,
                id: 0x1234_ABCD,
            },
        );
        let mut raw = [0u8; 32];
        let len = duplicate_set_address.encode(&mut raw).unwrap();

        let seen = harness.client.read_tick(&raw[..len]).await.unwrap();
        assert!(seen.is_none());

        let ack_bytes = harness.client.write_tick().await.unwrap();
        let ack = decode_single_encoded_frame(&ack_bytes).unwrap();
        assert_eq!(ack.payload(), &FramePayload::Ack(0x02));
        assert_eq!(ack.from_address(), 0x02);
        assert_eq!(ack.to_address(), Address::Unicast(0x01));
    });
}

#[test]
fn different_join_is_ignored_while_assignment_is_pending_on_os() {
    block_on(async {
        let mut harness = new_harness();

        harness.client.send_join(0xAAAA_0001).await.unwrap();
        let first_join_bytes = harness.client.write_tick().await.unwrap();
        let first_seen = harness.master.read_tick(&first_join_bytes).await.unwrap();
        assert!(matches!(
            first_seen.unwrap().payload(),
            FramePayload::Join(0xAAAA_0001)
        ));

        let mut other_client = new_harness().client;
        other_client.send_join(0xBBBB_0002).await.unwrap();
        let second_join_bytes = other_client.write_tick().await.unwrap();

        let second_seen = harness.master.read_tick(&second_join_bytes).await.unwrap();
        assert!(second_seen.is_none());

        let set_address_bytes = harness.master.write_tick().await.unwrap();
        let set_address = decode_single_encoded_frame(&set_address_bytes).unwrap();
        assert!(matches!(
            set_address.payload(),
            FramePayload::SetAddress {
                address: 0x02,
                id: 0xAAAA_0001,
            }
        ));
    });
}

#[test]
fn broadcast_ping_is_ponged_on_os() {
    block_on(async {
        let mut harness = new_harness();

        let frame = Frame::new(Address::Broadcast, 0x44, FramePayload::Ping);
        let mut raw = [0u8; 32];
        let len = frame.encode(&mut raw).unwrap();

        let seen = harness.client.read_tick(&raw[..len]).await.unwrap();
        assert!(matches!(seen, Some(ref frame) if frame.payload() == &FramePayload::Ping));

        let pong_bytes = harness.client.write_tick().await.unwrap();
        let pong = decode_single_encoded_frame(&pong_bytes).unwrap();
        assert_eq!(pong.payload(), &FramePayload::Pong);
        assert_eq!(pong.from_address(), 0x00);
        assert_eq!(pong.to_address(), Address::Unicast(0x44));
    });
}

#[test]
fn broadcast_set_is_acknowledged_on_os() {
    block_on(async {
        let mut harness = new_harness();
        join_client(&mut harness, 0x1111_2222).await;

        let frame = Frame::new(
            Address::Broadcast,
            0x01,
            FramePayload::Set(heapless::Vec::from_slice(&[0xAA, 0xBB]).unwrap()),
        );
        let mut raw = [0u8; 32];
        let len = frame.encode(&mut raw).unwrap();

        let seen = harness.client.read_tick(&raw[..len]).await.unwrap();
        assert!(matches!(
            seen,
            Some(ref frame) if frame.payload()
                == &FramePayload::Set(heapless::Vec::from_slice(&[0xAA, 0xBB]).unwrap())
        ));

        let ack_bytes = harness.client.write_tick().await.unwrap();
        let ack = decode_single_encoded_frame(&ack_bytes).unwrap();
        assert_eq!(ack.payload(), &FramePayload::Ack(0xFF));

        let master_seen = harness.master.read_tick(&ack_bytes).await.unwrap().unwrap();
        assert_eq!(master_seen.payload(), &FramePayload::Ack(0xFF));
    });
}

#[test]
fn client_rejects_set_address_when_not_ready_on_os() {
    block_on(async {
        let mut harness = new_harness();

        let frame = Frame::new(
            Address::Unicast(0x00),
            0x01,
            FramePayload::SetAddress {
                address: 0x02,
                id: 0x1111_2222,
            },
        );
        let mut raw = [0u8; 32];
        let len = frame.encode(&mut raw).unwrap();

        let result = harness.client.read_tick(&raw[..len]).await;

        assert_eq!(
            result,
            Err(ImcpError::ProtocolError(ProtocolError::NodeNotReady))
        );
    });
}

#[test]
fn master_rejects_set_address_frame_on_os() {
    block_on(async {
        let mut harness = new_harness();

        let frame = Frame::new(
            Address::Unicast(0x01),
            0x02,
            FramePayload::SetAddress {
                address: 0x02,
                id: 0x1111_2222,
            },
        );
        let mut raw = [0u8; 32];
        let len = frame.encode(&mut raw).unwrap();

        let result = harness.master.read_tick(&raw[..len]).await;

        assert_eq!(
            result,
            Err(ImcpError::ProtocolError(ProtocolError::InvalidFrameType(
                imcp::frame::FrameType::SetAddress
            )))
        );
    });
}

#[test]
fn client_rejects_join_frame_on_os() {
    block_on(async {
        let mut harness = new_harness();

        let frame = Frame::new(Address::Unicast(0x00), 0x01, FramePayload::Join(0x1111_2222));
        let mut raw = [0u8; 32];
        let len = frame.encode(&mut raw).unwrap();

        let result = harness.client.read_tick(&raw[..len]).await;

        assert_eq!(
            result,
            Err(ImcpError::ProtocolError(ProtocolError::InvalidFrameType(
                imcp::frame::FrameType::Join
            )))
        );
    });
}

#[test]
fn master_returns_duplicate_join_while_assignment_is_pending_on_os() {
    block_on(async {
        let mut harness = new_harness();

        harness.client.send_join(0xAAAA_0001).await.unwrap();
        let join_bytes = harness.client.write_tick().await.unwrap();

        let first = harness.master.read_tick(&join_bytes).await.unwrap();
        assert!(matches!(first, Some(ref frame) if frame.payload() == &FramePayload::Join(0xAAAA_0001)));

        let second = harness.master.read_tick(&join_bytes).await.unwrap();
        assert!(matches!(second, Some(ref frame) if frame.payload() == &FramePayload::Join(0xAAAA_0001)));
    });
}

#[test]
fn unexpected_ack_without_pending_is_rejected_on_os() {
    block_on(async {
        let mut harness = new_harness();

        let frame = Frame::new(Address::Unicast(0x00), 0x03, FramePayload::Ack(0x03));
        let mut raw = [0u8; 32];
        let len = frame.encode(&mut raw).unwrap();

        let result = harness.client.read_tick(&raw[..len]).await;

        assert_eq!(
            result,
            Err(ImcpError::ProtocolError(ProtocolError::UnexpectedAck))
        );
    });
}
