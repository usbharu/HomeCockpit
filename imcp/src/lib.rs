#![cfg_attr(all(not(test), not(feature = "test-utils")), no_std)]

use heapless::Vec;

use crate::channel::Receiver;
use crate::channel::Sender;
use crate::error::*;
use crate::frame::*;
use crate::parser::FrameParser;
pub mod channel;
pub mod error;
pub mod frame;
pub mod parser;

pub const SOF: u8 = 0xFE;
pub const EOF: u8 = 0xFF;
pub const ESC: u8 = 0xFD;

pub const ESC_XOR: u8 = 0x20;

#[cfg(feature = "defmt")]
use defmt::{info, trace}; // Format トレイトもインポート

// "defmt" フィーチャーが無効な場合 (ログを出力しない)
#[cfg(not(feature = "defmt"))]
mod defmt_dummy {
    // defmt::Format を実装できるようにダミーのトレイトを定義
    #[allow(dead_code)]
    pub trait Format {}
    impl<T> Format for &T {}
    impl<T> Format for &mut T {}
    // 他、必要な型に対してもダミー実装を追加
    impl Format for u32 {}
    impl Format for bool {}
    // ...

    // ログマクロを「何もしない」ものとして定義
    #[macro_export]
    macro_rules! info {
        ($($arg:tt)*) => {
            #[cfg(feature="test-utils")]
            println!("[INFO] {}", format_args!($($arg)*));
        };
    }
    #[macro_export]
    macro_rules! warn {
        ($($arg:tt)*) => {
            #[cfg(feature="test-utils")]
            println!("[WARN] {}", format_args!($($arg)*));
        };
    }
    #[macro_export]
    macro_rules! trace {
        ($($arg:tt)*) => {
            #[cfg(feature="test-utils")]
             println!("[TRACE] {}", format_args!($($arg)*));
        };
    }
}

#[derive(PartialEq)]
pub enum ClientState {
    NotReady,
    Joining(u32),
    Ready(u32),
}

#[derive(PartialEq)]
pub struct MasterState {
    next_address: u8,
    pending_assignment: Option<(u32, u8)>,
    pending_assignment_retries: u8,
}

#[derive(PartialEq)]
pub enum NodeType {
    Client(ClientState),
    Master(MasterState),
}

pub struct Imcp<'rx_buf, 'parser_frame_buffer, R, S> {
    tx_receiver: R,
    tx_sender: S,
    address: u8,
    node_id: Option<u32>,
    pending_frame: Option<Frame>,
    frame_parser: FrameParser<'rx_buf, 'parser_frame_buffer>,
    node_type: NodeType,
}

const MAX_SET_ADDRESS_RETRIES: u8 = 3;

impl MasterState {
    fn allocate_address(&self) -> Result<u8, ProtocolError> {
        if self.next_address == 0x00 || self.next_address == 0x01 || self.next_address == 0xFF {
            return Err(ProtocolError::AddressPoolExhausted);
        }
        Ok(self.next_address)
    }

    fn advance_address(&mut self) -> Result<(), ProtocolError> {
        if self.next_address >= 0xFE {
            self.next_address = 0xFF;
            return Err(ProtocolError::AddressPoolExhausted);
        }

        self.next_address = self.next_address.saturating_add(1);
        Ok(())
    }
}

impl<'rx_buf, 'parser_frame_buffer, R: Receiver, S: Sender>
    Imcp<'rx_buf, 'parser_frame_buffer, R, S>
{
    pub fn new_master(
        tx_receiver: R,
        tx_sender: S,
        rx_buffer: &'rx_buf mut [u8],
        parser_frame_buffer: &'parser_frame_buffer mut [u8],
    ) -> Self {
        let frame_parser = FrameParser::new(rx_buffer, parser_frame_buffer);
        info!("new master registered");
        Self {
            address: 0x01,
            node_id: None,
            pending_frame: None,
            frame_parser,
            node_type: NodeType::Master(MasterState {
                next_address: 0x02,
                pending_assignment: None,
                pending_assignment_retries: 0,
            }),
            tx_receiver,
            tx_sender,
        }
    }

    pub fn new_client(
        tx_receiver: R,
        tx_sender: S,
        rx_buffer: &'rx_buf mut [u8],
        parser_frame_buffer: &'parser_frame_buffer mut [u8],
    ) -> Self {
        let frame_parser = FrameParser::new(rx_buffer, parser_frame_buffer);
        info!("new client registered");
        Self {
            address: 0x00,
            node_id: None,
            pending_frame: None,
            frame_parser,
            node_type: NodeType::Client(ClientState::NotReady),
            tx_receiver,
            tx_sender,
        }
    }

    pub async fn send_join(&mut self, id: u32) -> Result<(), ImcpError<R::Error, S::Error>> {
        if let NodeType::Client(_state) = &self.node_type {
            self.node_id = Some(id);
            self.node_type = NodeType::Client(ClientState::Joining(id));
            let frame = Frame::new(Address::Unicast(0x01), self.address, FramePayload::Join(id));
            self.tx_sender
                .send(frame)
                .await
                .map_err(ImcpError::SendError)?;
        }
        Ok(())
    }

    pub async fn write_tick(
        &mut self,
    ) -> Result<Vec<u8, MAX_ENCODED_FRAME_SIZE>, ImcpError<R::Error, S::Error>> {
        let next_frame = loop {
            let frame = if let Some(frame) = self.pending_frame.take() {
                trace!("rewrite pending_frame: {:?}", frame);
                frame
            } else {
                trace!("wait for write new frame");
                self.tx_receiver
                    .receive()
                    .await
                    .map_err(ImcpError::ReceiveError)?
            };

            if matches!(frame.payload(), FramePayload::SetAddress { .. })
                && let NodeType::Master(state) = &mut self.node_type
            {
                if state.pending_assignment_retries >= MAX_SET_ADDRESS_RETRIES {
                    state.pending_assignment = None;
                    state.pending_assignment_retries = 0;
                    continue;
                }
                state.pending_assignment_retries =
                    state.pending_assignment_retries.saturating_add(1);
            }

            break frame;
        };
        let mut raw = [0u8; MAX_ENCODED_FRAME_SIZE];
        let size = next_frame
            .encode(&mut raw)
            .map_err(ImcpError::EncodeError)?;
        let mut buf = Vec::<u8, MAX_ENCODED_FRAME_SIZE>::new();
        buf.extend_from_slice(&raw[..size])
            .map_err(|_| ImcpError::EncodeError(EncodeError::BufferTooSmall))?;
        match next_frame.payload() {
            FramePayload::SetAddress { address: _, id: _ } => {
                trace!("set pending_frame to {:?}", next_frame);
                self.pending_frame = Some(next_frame);
            }
            FramePayload::Join(_id) => {
                trace!("set pending_frame to {:?}", next_frame);
                self.pending_frame = Some(next_frame);
            }
            FramePayload::Set(_vec_inner) => {
                trace!("set pending_frame to {:?}", next_frame);
                self.pending_frame = Some(next_frame);
            }
            _ => {}
        }
        Ok(buf)
    }

    pub async fn read_tick<'b>(
        &'b mut self,
        new_data: &[u8],
    ) -> Result<Option<Frame>, error::ImcpError<R::Error, S::Error>>
    where
        'parser_frame_buffer: 'b,
    {
        self.frame_parser
            .write_data(new_data)
            .map_err(ImcpError::DecodeError)?;
        let mut frame = match self.frame_parser.next_frame() {
            Some(Ok(f)) => f,
            Some(Err(e)) => return Err(ImcpError::DecodeError(e)),
            None => return Ok(None),
        };

        match frame.to_address() {
            Address::Unicast(a) => {
                if a != self.address {
                    return Ok(None);
                }
            }
            Address::Broadcast => (),
        }

        match frame.payload_mut() {
            FramePayload::Ack(data) => {
                if self.pending_frame.is_none() && data != &0xFF {
                    return Err(ImcpError::ProtocolError(ProtocolError::UnexpectedAck));
                }
                if let Some(pending_frame) = self.pending_frame.as_ref() {
                    let expected_address = pending_frame.to_address().as_byte();
                    let expected_sender = match pending_frame.payload() {
                        FramePayload::SetAddress { address, .. } => *address,
                        _ => expected_address,
                    };
                    if *data != expected_address {
                        return Err(ImcpError::ProtocolError(ProtocolError::UnexpectedAck));
                    }

                    if !matches!(pending_frame.to_address(), Address::Broadcast)
                        && frame.from_address() != expected_sender
                    {
                        return Err(ImcpError::ProtocolError(ProtocolError::UnexpectedAck));
                    }

                    if matches!(pending_frame.payload(), FramePayload::SetAddress { .. })
                        && let NodeType::Master(state) = &mut self.node_type
                    {
                        state.pending_assignment = None;
                        state.pending_assignment_retries = 0;
                        let _ = state.advance_address();
                    }
                }

                self.pending_frame = None;
            }
            FramePayload::SetAddress { address, id } => {
                if let NodeType::Master(_) = self.node_type {
                    return Err(ImcpError::ProtocolError(ProtocolError::InvalidFrameType(
                        FrameType::SetAddress,
                    )));
                }

                if let NodeType::Client(state) = &self.node_type {
                    match state {
                        ClientState::Joining(own_id) => {
                            if id != own_id {
                                return Ok(None);
                            }
                            let assigned_address = *address;
                            let own_id = *own_id;
                            self.address = assigned_address;
                            self.pending_frame = None;
                            self.node_type = NodeType::Client(ClientState::Ready(own_id));
                            self.tx_sender
                                .send(Frame::new(
                                    Address::Unicast(frame.from_address()),
                                    self.address,
                                    FramePayload::Ack(frame.to_address().as_byte()),
                                ))
                                .await
                                .map_err(ImcpError::SendError)?;
                        }
                        ClientState::Ready(own_id) => {
                            if self.address != *address || own_id != id {
                                return Err(ImcpError::ProtocolError(
                                    ProtocolError::InvalidFrameType(FrameType::SetAddress),
                                ));
                            }
                            self.tx_sender
                                .send(Frame::new(
                                    Address::Unicast(frame.from_address()),
                                    self.address,
                                    FramePayload::Ack(frame.to_address().as_byte()),
                                ))
                                .await
                                .map_err(ImcpError::SendError)?;
                            return Ok(None);
                        }
                        ClientState::NotReady => {
                            return Err(ImcpError::ProtocolError(ProtocolError::NodeNotReady));
                        }
                    }
                }
            }
            FramePayload::Join(id) => {
                if let NodeType::Master(state) = &mut self.node_type {
                    if let Some((pending_id, _)) = state.pending_assignment {
                        if pending_id == *id {
                            return Ok(Some(frame));
                        }
                        return Ok(None);
                    }

                    let assigned_address = state
                        .allocate_address()
                        .map_err(ImcpError::ProtocolError)?;
                    let frame = Frame::new(
                        Address::Unicast(0x00),
                        self.address,
                        FramePayload::SetAddress {
                            address: assigned_address,
                            id: *id,
                        },
                    );
                    state.pending_assignment = Some((*id, assigned_address));
                    state.pending_assignment_retries = 0;
                    self.tx_sender
                        .send(frame)
                        .await
                        .map_err(ImcpError::SendError)?;
                } else {
                    return Err(ImcpError::ProtocolError(ProtocolError::InvalidFrameType(
                        FrameType::Join,
                    )));
                }
            }
            FramePayload::Set(_data) => {
                self.tx_sender
                    .send(Frame::new(
                        Address::Unicast(frame.from_address()),
                        self.address,
                        FramePayload::Ack(frame.to_address().as_byte()),
                    ))
                    .await
                    .map_err(ImcpError::SendError)?;
            }
            FramePayload::Ping => {
                self.tx_sender
                    .send(Frame::new(
                        Address::Unicast(frame.from_address()),
                        self.address,
                        FramePayload::Pong,
                    ))
                    .await
                    .map_err(ImcpError::SendError)?;
            }
            _ => {}
        };
        Ok(Some(frame))
    }
}

#[cfg(feature = "test-utils")]
pub mod imcp_test {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use crate::{
        DecodeError, Imcp, NodeType,
        channel::*,
        frame::{Frame, MAX_ENCODED_FRAME_SIZE},
        parser::FrameParser,
    };

    #[derive(Clone)]
    pub struct MemorySender {
        frames: Arc<Mutex<VecDeque<Frame>>>,
    }

    pub struct MemoryReceiver {
        frames: Arc<Mutex<VecDeque<Frame>>>,
    }

    pub fn memory_channel() -> (MemorySender, MemoryReceiver) {
        let frames = Arc::new(Mutex::new(VecDeque::new()));
        (
            MemorySender {
                frames: Arc::clone(&frames),
            },
            MemoryReceiver { frames },
        )
    }

    impl Sender for MemorySender {
        type Error = std::convert::Infallible;

        async fn send(&mut self, frame: Frame) -> Result<(), Self::Error> {
            self.frames.lock().unwrap().push_back(frame);
            Ok(())
        }
    }

    impl Receiver for MemoryReceiver {
        type Error = std::convert::Infallible;

        async fn receive(&mut self) -> Result<Frame, Self::Error> {
            Ok(self
                .frames
                .lock()
                .unwrap()
                .pop_front()
                .expect("memory receiver should have a frame"))
        }
    }

    pub fn decode_single_encoded_frame(bytes: &[u8]) -> Result<Frame, DecodeError> {
        let mut rx_buf = [0u8; MAX_ENCODED_FRAME_SIZE];
        let mut frame_buf = [0u8; MAX_ENCODED_FRAME_SIZE];
        let mut parser = FrameParser::new(&mut rx_buf, &mut frame_buf);
        parser.write_data(bytes)?;
        parser
            .next_frame()
            .expect("encoded frame should contain a complete frame")
    }

    impl<'rx_buf, 'parser_frame_buffer, R: Receiver, S: Sender>
        Imcp<'rx_buf, 'parser_frame_buffer, R, S>
    {
        pub fn new(
            tx_receiver: R,
            tx_sender: S,
            address: u8,
            pending_frame: Option<Frame>,
            frame_parser: FrameParser<'rx_buf, 'parser_frame_buffer>,
            node_type: NodeType,
        ) -> Imcp<'rx_buf, 'parser_frame_buffer, R, S> {
            Imcp {
                tx_receiver,
                tx_sender,
                address,
                node_id: None,
                pending_frame,
                frame_parser,
                node_type,
            }
        }

        pub fn address(&self) -> u8 {
            self.address
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used,clippy::expect_used)]
mod tests {

    use core::convert::Infallible;

    use heapless::Vec;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::parser::FrameParser;

    #[derive(Default)]
    struct TestSender {
        sent: std::vec::Vec<Frame>,
    }

    impl Sender for TestSender {
        type Error = Infallible;

        async fn send(&mut self, frame: Frame) -> Result<(), Self::Error> {
            self.sent.push(frame);
            Ok(())
        }
    }

    struct TestReceiver {
        frames: std::collections::VecDeque<Frame>,
    }

    impl TestReceiver {
        fn new(frames: impl IntoIterator<Item = Frame>) -> Self {
            Self {
                frames: frames.into_iter().collect(),
            }
        }
    }

    impl Receiver for TestReceiver {
        type Error = Infallible;

        async fn receive(&mut self) -> Result<Frame, Self::Error> {
            Ok(self.frames.pop_front().expect("test receiver should have a frame"))
        }
    }

    #[derive(Clone)]
    struct QueueSender {
        frames: Arc<Mutex<VecDeque<Frame>>>,
    }

    impl Sender for QueueSender {
        type Error = Infallible;

        async fn send(&mut self, frame: Frame) -> Result<(), Self::Error> {
            self.frames.lock().unwrap().push_back(frame);
            Ok(())
        }
    }

    struct QueueReceiver {
        frames: Arc<Mutex<VecDeque<Frame>>>,
    }

    impl Receiver for QueueReceiver {
        type Error = Infallible;

        async fn receive(&mut self) -> Result<Frame, Self::Error> {
            Ok(self
                .frames
                .lock()
                .unwrap()
                .pop_front()
                .expect("queue receiver should have a frame"))
        }
    }

    fn encode_frame(frame: &Frame) -> std::vec::Vec<u8> {
        let mut raw = [0u8; MAX_ENCODED_FRAME_SIZE];
        let len = frame.encode(&mut raw).unwrap();
        raw[..len].to_vec()
    }

    #[test]
    fn test_encode_buffer_too_small() {
        let data: &[u8] = &[0x01, 0x02];
        let payload = FramePayload::Data(Vec::from_slice(data).unwrap());
        let frame = Frame::new(Address::Unicast(0x01), 0x02, payload);

        let mut buffer = [0u8; 7];
        let result = frame.encode(&mut buffer);

        assert_eq!(result, Err(EncodeError::BufferTooSmall));
    }

    #[test]
    fn test_encode_stuffed_ping() {
        let frame = Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ping);
        let mut buffer = [0u8; 32];
        let len = frame.encode(&mut buffer).unwrap();

        // SOF [H+P+C] EOF
        // H = 01 02 00 00 00
        // P = (empty)
        // C = 01^02^00^00^00 = 03
        // [H+P+C] = 01 02 00 00 00 03 (エスケープ対象なし)
        // Final = FE [01 02 00 00 00 03] FF
        let expected: &[u8] = &[0xFE, 0x01, 0x02, 0x00, 0x00, 0x00, 0x03, 0xFF];
        assert_eq!(len, expected.len());
        assert_eq!(&buffer[..len], expected);
    }

    #[test]
    fn test_encode_with_stuffing() {
        // わざとエスケープ対象 (0xFE) をペイロードに入れる
        let data: &[u8] = &[0x01, SOF, 0x03]; // 01 FE 03
        let frame = Frame::new(
            Address::Unicast(0x01),
            0x02,
            FramePayload::Data(Vec::from_slice(data).unwrap()),
        );

        // H = 01 02 05 03 00 (Type=Data, Len=3)
        // P = 01 FE 03
        // C = (01^02^05^03^00) ^ (01^FE^03) = 0x05 ^ 0xFD = 0xF8
        // [H+P+C] = 01 02 05 03 00 | 01 FE 03 | F8

        // Stuffing:
        // 01 02 05 03 00 | 01 [FD DE] 03 | F8

        // Final:
        // FE [01 02 05 03 00 01 FD DE 03 F8] FF
        let expected: &[u8] = &[
            SOF,
            0x01,
            0x02,
            0x05,
            0x03,
            0x00, // Header
            0x01,
            ESC,
            SOF ^ ESC_XOR,
            0x03, // Payload (stuffed)
            0xF9, // Checksum
            EOF,
        ];

        let mut buffer = [0u8; 32];
        let len = frame.encode(&mut buffer).unwrap();

        assert_eq!(len, expected.len());
        assert_eq!(&buffer[..len], expected);
    }

    // --- (Decode テスト (純粋フレーム)) ---
    #[test]
    fn test_decode_pure_frame() {
        // H+P+C のみ
        let buffer: &[u8] = &[0x01, 0x02, 0x00, 0x00, 0x00, 0x03]; // Ping
        let frame = Frame::decode(buffer).unwrap();
        assert_eq!(frame.payload(), &FramePayload::Ping);
    }

    #[test]
    fn test_decode_error_bad_checksum_pure() {
        let buffer: &[u8] = &[0x01, 0x02, 0x00, 0x00, 0x00, 0xFF]; // Bad Checksum
        let res = Frame::decode(buffer);
        assert_eq!(res, Err(DecodeError::InvalidChecksum));
    }

    // --- (FrameParser テスト) ---
    #[test]
    fn test_parser_stuffed_frame() {
        let mut rx_buf = [0u8; 64];
        let mut frame_buf = [0u8; 64];
        let mut parser = FrameParser::new(&mut rx_buf, &mut frame_buf);

        // test_encode_with_stuffing のデータ
        let stuffed_frame: &[u8] = &[
            SOF,
            0x01,
            0x02,
            0x05,
            0x03,
            0x00,
            0x01,
            ESC,
            SOF ^ ESC_XOR,
            0x03,
            0xF9,
            EOF,
        ];
        println!("{:X?}", stuffed_frame);

        parser.write_data(stuffed_frame).unwrap();

        let frame_res = parser.next_frame().unwrap();
        let frame = frame_res.unwrap();

        let expected_data: &[u8] = &[0x01, SOF, 0x03];
        assert_eq!(
            frame.payload(),
            &FramePayload::Data(Vec::from_slice(expected_data).unwrap())
        );
        assert!(parser.next_frame().is_none());
    }

    #[test]
    fn test_parser_sync_loss_recovery_with_sof() {
        let mut rx_buf = [0u8; 64];
        let mut frame_buf = [0u8; 64];
        let mut parser = FrameParser::new(&mut rx_buf, &mut frame_buf);

        let junk: &[u8] = &[0xAA, 0xBB, 0xCC];
        let frame1: &[u8] = &[SOF, 0x01, 0x02, 0x00, 0x00, 0x00, 0x03, EOF]; // Ping
        let frame2: &[u8] = &[SOF, 0x01, 0x03, 0x00, 0x00, 0x00, 0x02, EOF]; // Ping (from 03)

        parser.write_data(junk).unwrap();
        parser.write_data(frame1).unwrap();
        parser.write_data(frame2).unwrap();

        // 1つ目 (Junk は無視される)
        let f1 = parser.next_frame().unwrap().unwrap();
        assert_eq!(f1.from_address(), 0x02);

        // 2つ目
        let f2 = parser.next_frame().unwrap().unwrap();
        assert_eq!(f2.from_address(), 0x03);

        assert!(parser.next_frame().is_none());
    }

    #[test]
    fn test_parser_incomplete_stuffed() {
        let mut rx_buf = [0u8; 64];
        let mut frame_buf = [0u8; 64];
        let mut parser = FrameParser::new(&mut rx_buf, &mut frame_buf);

        // [SOF, 0x01, 0x02, 0x05, 0x03, 0x00, 0x01, ESC] ... まで
        let part1: &[u8] = &[SOF, 0x01, 0x02, 0x05, 0x03, 0x00, 0x01, ESC];
        parser.write_data(part1).unwrap();

        // まだフレームは完成しない
        assert!(parser.next_frame().is_none());

        // ... [SOF^ESC_XOR, 0x03, 0xF8, EOF]
        let part2: &[u8] = &[SOF ^ ESC_XOR, 0x03, 0xF9, EOF];
        parser.write_data(part2).unwrap();

        // 今度は完成する
        let frame = parser.next_frame().unwrap().unwrap();
        let expected_data: &[u8] = &[0x01, SOF, 0x03];
        assert_eq!(
            frame.payload(),
            &FramePayload::Data(Vec::from_slice(expected_data).unwrap())
        );
        assert!(parser.next_frame().is_none());
    }

    #[test]
    fn test_roundtrip_encode_decode_with_stuffing() {
        // 1. Arrange (準備)

        // エスケープ対象バイトをすべて含むペイロードを作成
        let test_data: &[u8] = &[0x01, SOF, 0x03, EOF, 0x05, ESC, 0x07];
        let original_frame = Frame::new(
            Address::Broadcast,
            0x42,
            FramePayload::Data(Vec::from_slice(test_data).unwrap()),
        );

        // エンコード用バッファ (最大長を確保)
        let mut encode_buffer = [0u8; 128];

        // デコード用パーサーのバッファ
        let mut rx_buf = [0u8; 128];
        let mut frame_buf = [0u8; 128];
        let mut parser = FrameParser::new(&mut rx_buf, &mut frame_buf);

        // 2. Act (実行)

        // フレームをエンコード
        let encoded_len = original_frame.encode(&mut encode_buffer).unwrap();
        let encoded_slice = &encode_buffer[..encoded_len];

        // エンコードされたデータをパーサーに書き込む
        parser.write_data(encoded_slice).unwrap();

        // パーサーからフレームをデコード
        let decode_result = parser.next_frame();

        // 3. Assert (検証)

        // フレームが1つ正常にデコードされたか
        assert!(
            decode_result.is_some(),
            "フレームがデコードされませんでした"
        );
        let decoded_frame = decode_result
            .unwrap()
            .expect("デコード中にエラーが発生しました");

        // 元のフレームとデコードされたフレームが一致するか
        assert_eq!(
            original_frame, decoded_frame,
            "エンコード前後でフレームが一致しません"
        );

        // パーサーに余分なデータが残っていないか
        assert!(
            parser.next_frame().is_none(),
            "パーサーに余分なフレームが残っています"
        );
    }

    #[test]
    fn test_encode_max_payload_with_stuffing_fits_max_encoded_frame_size() {
        let payload = [SOF; MAX_PAYLOAD_SIZE];
        let frame = Frame::new(
            Address::Broadcast,
            0x42,
            FramePayload::Data(Vec::from_slice(&payload).unwrap()),
        );
        let mut buffer = [0u8; MAX_ENCODED_FRAME_SIZE];

        let encoded_len = frame.encode(&mut buffer).unwrap();

        assert!(encoded_len <= MAX_ENCODED_FRAME_SIZE);
        assert!(encoded_len > MAX_PAYLOAD_SIZE);
    }

    #[test]
    fn test_send_join_targets_master_address() {
        futures::executor::block_on(async {
            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut imcp = Imcp::new_client(receiver, sender, &mut rx_buf, &mut frame_buf);

            imcp.send_join(0x1234_5678).await.unwrap();

            let join = imcp.tx_sender.sent.first().unwrap();
            assert_eq!(join.to_address(), Address::Unicast(0x01));
            assert_eq!(join.from_address(), 0x00);
            assert_eq!(join.payload(), &FramePayload::Join(0x1234_5678));
        });
    }

    #[test]
    fn test_pending_frame_survives_mismatched_ack() {
        futures::executor::block_on(async {
            let pending_frame = Frame::new(
                Address::Unicast(0x02),
                0x01,
                FramePayload::Set(Vec::from_slice(&[0x10, 0x20]).unwrap()),
            );
            let mut pending_bytes = [0u8; 32];
            let _ = pending_frame.encode(&mut pending_bytes).unwrap();

            let ack_frame = Frame::new(Address::Unicast(0x01), 0x03, FramePayload::Ack(0x03));
            let mut encoded = [0u8; 32];
            let encoded_len = ack_frame.encode(&mut encoded).unwrap();

            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let parser = FrameParser::new(&mut rx_buf, &mut frame_buf);
            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut imcp = Imcp {
                tx_receiver: receiver,
                tx_sender: sender,
                address: 0x01,
                node_id: None,
                pending_frame: Some(pending_frame),
                frame_parser: parser,
                node_type: NodeType::Master(MasterState {
                    next_address: 0x02,
                    pending_assignment: None,
                    pending_assignment_retries: 0,
                }),
            };

            let result = imcp.read_tick(&encoded[..encoded_len]).await;

            assert_eq!(
                result,
                Err(ImcpError::ProtocolError(ProtocolError::UnexpectedAck))
            );
            assert!(imcp.pending_frame.is_some());
        });
    }

    #[test]
    fn test_write_tick_marks_join_for_retry() {
        futures::executor::block_on(async {
            let shared = Arc::new(Mutex::new(VecDeque::new()));
            let receiver = QueueReceiver {
                frames: Arc::clone(&shared),
            };
            let sender = QueueSender { frames: shared };
            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let mut imcp = Imcp::new_client(receiver, sender, &mut rx_buf, &mut frame_buf);

            imcp.send_join(0x1234_5678).await.unwrap();
            let encoded = imcp.write_tick().await.unwrap();

            let mut parser_rx = [0u8; 64];
            let mut parser_frame = [0u8; 64];
            let mut parser = FrameParser::new(&mut parser_rx, &mut parser_frame);
            parser.write_data(&encoded).unwrap();
            let frame = parser.next_frame().unwrap().unwrap();

            assert_eq!(frame.payload(), &FramePayload::Join(0x1234_5678));
            assert_eq!(
                imcp.pending_frame.as_ref().map(Frame::payload),
                Some(&FramePayload::Join(0x1234_5678))
            );

            let encoded_retry = imcp.write_tick().await.unwrap();
            let mut parser_rx = [0u8; 64];
            let mut parser_frame = [0u8; 64];
            let mut parser = FrameParser::new(&mut parser_rx, &mut parser_frame);
            parser.write_data(&encoded_retry).unwrap();
            let retry_frame = parser.next_frame().unwrap().unwrap();

            assert_eq!(retry_frame.payload(), &FramePayload::Join(0x1234_5678));
            assert_eq!(
                imcp.pending_frame.as_ref().map(Frame::payload),
                Some(&FramePayload::Join(0x1234_5678))
            );
            assert_eq!(imcp.tx_receiver.frames.lock().unwrap().len(), 0);
        });
    }

    #[test]
    fn test_write_tick_marks_set_for_retry() {
        futures::executor::block_on(async {
            let shared = Arc::new(Mutex::new(VecDeque::new()));
            let receiver = QueueReceiver {
                frames: Arc::clone(&shared),
            };
            let sender = QueueSender { frames: shared };
            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let set = Frame::new(
                Address::Unicast(0x02),
                0x01,
                FramePayload::Set(Vec::from_slice(&[0x10, 0x20]).unwrap()),
            );
            let mut imcp = Imcp {
                tx_receiver: receiver,
                tx_sender: sender,
                address: 0x01,
                node_id: None,
                pending_frame: Some(set.clone()),
                frame_parser: FrameParser::new(&mut rx_buf, &mut frame_buf),
                node_type: NodeType::Master(MasterState {
                    next_address: 0x02,
                    pending_assignment: None,
                    pending_assignment_retries: 0,
                }),
            };

            let encoded = imcp.write_tick().await.unwrap();

            let mut parser_rx = [0u8; 64];
            let mut parser_frame = [0u8; 64];
            let mut parser = FrameParser::new(&mut parser_rx, &mut parser_frame);
            parser.write_data(&encoded).unwrap();
            let frame = parser.next_frame().unwrap().unwrap();

            assert_eq!(frame.payload(), set.payload());
            assert_eq!(imcp.pending_frame.as_ref().map(Frame::payload), Some(set.payload()));
            assert_eq!(imcp.tx_receiver.frames.lock().unwrap().len(), 0);
        });
    }

    #[test]
    fn test_write_tick_drops_stale_set_address_after_retry_limit() {
        futures::executor::block_on(async {
            let retry_exhausted = Frame::new(
                Address::Unicast(0x00),
                0x01,
                FramePayload::SetAddress {
                    address: 0x02,
                    id: 0xAA55_AA55,
                },
            );
            let fallback = Frame::new(Address::Broadcast, 0x01, FramePayload::Ping);

            let receiver = TestReceiver::new([fallback.clone()]);
            let sender = TestSender::default();
            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let parser = FrameParser::new(&mut rx_buf, &mut frame_buf);
            let mut imcp = Imcp {
                tx_receiver: receiver,
                tx_sender: sender,
                address: 0x01,
                node_id: None,
                pending_frame: Some(retry_exhausted),
                frame_parser: parser,
                node_type: NodeType::Master(MasterState {
                    next_address: 0x02,
                    pending_assignment: Some((0xAA55_AA55, 0x02)),
                    pending_assignment_retries: MAX_SET_ADDRESS_RETRIES,
                }),
            };

            let encoded = imcp.write_tick().await.unwrap();

            let mut parser_rx = [0u8; 64];
            let mut parser_frame = [0u8; 64];
            let mut parser = FrameParser::new(&mut parser_rx, &mut parser_frame);
            parser.write_data(&encoded).unwrap();
            let frame = parser.next_frame().unwrap().unwrap();

            assert_eq!(frame.payload(), fallback.payload());
            assert!(imcp.pending_frame.is_none());
            assert!(matches!(
                imcp.node_type,
                NodeType::Master(MasterState {
                    next_address: 0x02,
                    pending_assignment: None,
                    pending_assignment_retries: 0,
                })
            ));
        });
    }

    #[test]
    fn test_write_tick_retries_set_address_before_retry_limit() {
        futures::executor::block_on(async {
            let pending_set_address = Frame::new(
                Address::Unicast(0x00),
                0x01,
                FramePayload::SetAddress {
                    address: 0x02,
                    id: 0xAA55_AA55,
                },
            );

            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let parser = FrameParser::new(&mut rx_buf, &mut frame_buf);
            let mut imcp = Imcp {
                tx_receiver: receiver,
                tx_sender: sender,
                address: 0x01,
                node_id: None,
                pending_frame: Some(pending_set_address.clone()),
                frame_parser: parser,
                node_type: NodeType::Master(MasterState {
                    next_address: 0x02,
                    pending_assignment: Some((0xAA55_AA55, 0x02)),
                    pending_assignment_retries: MAX_SET_ADDRESS_RETRIES - 1,
                }),
            };

            let encoded = imcp.write_tick().await.unwrap();

            let mut parser_rx = [0u8; 64];
            let mut parser_frame = [0u8; 64];
            let mut parser = FrameParser::new(&mut parser_rx, &mut parser_frame);
            parser.write_data(&encoded).unwrap();
            let frame = parser.next_frame().unwrap().unwrap();

            assert_eq!(frame.payload(), pending_set_address.payload());
            assert!(imcp.pending_frame.is_some());
            assert!(matches!(
                imcp.node_type,
                NodeType::Master(MasterState {
                    next_address: 0x02,
                    pending_assignment: Some((0xAA55_AA55, 0x02)),
                    pending_assignment_retries: MAX_SET_ADDRESS_RETRIES,
                })
            ));
        });
    }

    #[test]
    fn test_read_tick_accepts_broadcast_ack_without_pending_frame() {
        futures::executor::block_on(async {
            let ack_frame = Frame::new(Address::Unicast(0x01), 0x42, FramePayload::Ack(0xFF));
            let encoded = encode_frame(&ack_frame);

            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut imcp = Imcp::new_master(receiver, sender, &mut rx_buf, &mut frame_buf);

            let seen = imcp.read_tick(&encoded).await.unwrap().unwrap();

            assert_eq!(seen.payload(), &FramePayload::Ack(0xFF));
            assert!(imcp.pending_frame.is_none());
        });
    }

    #[test]
    fn test_read_tick_clears_pending_set_address_and_advances_master_state_on_ack() {
        futures::executor::block_on(async {
            let pending_frame = Frame::new(
                Address::Unicast(0x00),
                0x01,
                FramePayload::SetAddress {
                    address: 0x02,
                    id: 0x1122_3344,
                },
            );
            let ack_frame = Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ack(0x00));
            let encoded = encode_frame(&ack_frame);

            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let parser = FrameParser::new(&mut rx_buf, &mut frame_buf);
            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut imcp = Imcp {
                tx_receiver: receiver,
                tx_sender: sender,
                address: 0x01,
                node_id: None,
                pending_frame: Some(pending_frame),
                frame_parser: parser,
                node_type: NodeType::Master(MasterState {
                    next_address: 0x02,
                    pending_assignment: Some((0x1122_3344, 0x02)),
                    pending_assignment_retries: 2,
                }),
            };

            let seen = imcp.read_tick(&encoded).await.unwrap().unwrap();

            assert_eq!(seen.payload(), &FramePayload::Ack(0x00));
            assert!(imcp.pending_frame.is_none());
            assert!(matches!(
                imcp.node_type,
                NodeType::Master(MasterState {
                    next_address: 0x03,
                    pending_assignment: None,
                    pending_assignment_retries: 0,
                })
            ));
        });
    }

    #[test]
    fn test_read_tick_rejects_set_address_for_master() {
        futures::executor::block_on(async {
            let set_address = Frame::new(
                Address::Unicast(0x01),
                0x00,
                FramePayload::SetAddress {
                    address: 0x02,
                    id: 0xABCD_EF01,
                },
            );
            let encoded = encode_frame(&set_address);

            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut imcp = Imcp::new_master(receiver, sender, &mut rx_buf, &mut frame_buf);

            let result = imcp.read_tick(&encoded).await;

            assert_eq!(
                result,
                Err(ImcpError::ProtocolError(ProtocolError::InvalidFrameType(
                    FrameType::SetAddress,
                )))
            );
        });
    }

    #[test]
    fn test_read_tick_rejects_set_address_when_client_not_ready() {
        futures::executor::block_on(async {
            let set_address = Frame::new(
                Address::Unicast(0x00),
                0x01,
                FramePayload::SetAddress {
                    address: 0x02,
                    id: 0xABCD_EF01,
                },
            );
            let encoded = encode_frame(&set_address);

            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut imcp = Imcp::new_client(receiver, sender, &mut rx_buf, &mut frame_buf);

            let result = imcp.read_tick(&encoded).await;

            assert_eq!(
                result,
                Err(ImcpError::ProtocolError(ProtocolError::NodeNotReady))
            );
        });
    }

    #[test]
    fn test_read_tick_rejects_join_for_client() {
        futures::executor::block_on(async {
            let join = Frame::new(Address::Unicast(0x00), 0x01, FramePayload::Join(0x55AA_55AA));
            let encoded = encode_frame(&join);

            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut imcp = Imcp::new_client(receiver, sender, &mut rx_buf, &mut frame_buf);

            let result = imcp.read_tick(&encoded).await;

            assert_eq!(
                result,
                Err(ImcpError::ProtocolError(ProtocolError::InvalidFrameType(
                    FrameType::Join,
                )))
            );
        });
    }

    #[test]
    fn test_read_tick_returns_same_join_when_matching_pending_assignment_exists() {
        futures::executor::block_on(async {
            let join = Frame::new(Address::Unicast(0x01), 0x00, FramePayload::Join(0x55AA_55AA));
            let encoded = encode_frame(&join);

            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let parser = FrameParser::new(&mut rx_buf, &mut frame_buf);
            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut imcp = Imcp {
                tx_receiver: receiver,
                tx_sender: sender,
                address: 0x01,
                node_id: None,
                pending_frame: None,
                frame_parser: parser,
                node_type: NodeType::Master(MasterState {
                    next_address: 0x02,
                    pending_assignment: Some((0x55AA_55AA, 0x02)),
                    pending_assignment_retries: 1,
                }),
            };

            let seen = imcp.read_tick(&encoded).await.unwrap().unwrap();

            assert_eq!(seen.payload(), &FramePayload::Join(0x55AA_55AA));
            assert!(imcp.tx_sender.sent.is_empty());
            assert!(matches!(
                imcp.node_type,
                NodeType::Master(MasterState {
                    next_address: 0x02,
                    pending_assignment: Some((0x55AA_55AA, 0x02)),
                    pending_assignment_retries: 1,
                })
            ));
        });
    }

    #[test]
    fn test_read_tick_reports_address_pool_exhaustion_on_join() {
        futures::executor::block_on(async {
            let join = Frame::new(Address::Unicast(0x01), 0x00, FramePayload::Join(0x1234_5678));
            let encoded = encode_frame(&join);

            let mut rx_buf = [0u8; 64];
            let mut frame_buf = [0u8; 64];
            let parser = FrameParser::new(&mut rx_buf, &mut frame_buf);
            let receiver = TestReceiver::new(std::iter::empty());
            let sender = TestSender::default();
            let mut imcp = Imcp {
                tx_receiver: receiver,
                tx_sender: sender,
                address: 0x01,
                node_id: None,
                pending_frame: None,
                frame_parser: parser,
                node_type: NodeType::Master(MasterState {
                    next_address: 0xFF,
                    pending_assignment: None,
                    pending_assignment_retries: 0,
                }),
            };

            let result = imcp.read_tick(&encoded).await;

            assert_eq!(
                result,
                Err(ImcpError::ProtocolError(
                    ProtocolError::AddressPoolExhausted
                ))
            );
        });
    }
}
