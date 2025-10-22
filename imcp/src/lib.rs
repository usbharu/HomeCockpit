#![cfg_attr(not(test), no_std)]

use crate::error::*;
use crate::frame::*;
use crate::parser::FrameParser;
pub mod error;
pub mod frame;
pub mod parser;

pub const SOF: u8 = 0xFE;
pub const EOF: u8 = 0xFF;
pub const ESC: u8 = 0xFD;

const ESC_XOR: u8 = 0x20;

#[derive(PartialEq)]
enum ClientState{
    Joining(u32),
    Ready
}

#[derive(PartialEq)]
enum NodeType{
    Client(ClientState),
    Master
}

struct Imcp<'tx_buf,'rx_buf,'frame_buf,'parser_frame_buffer> {
    tx_buffer: &'tx_buf mut [u8],
    address: u8,
    pending_frame: Option<Frame<'frame_buf>>,
    frame_parser: FrameParser<'rx_buf,'parser_frame_buffer>,
    node_type:NodeType
}

impl<'tx_buf,'rx_buf,'frame_buf,'parser_frame_buffer> Imcp<'tx_buf,'rx_buf,'frame_buf,'parser_frame_buffer> {
    pub fn new_master(tx_buffer: &'tx_buf mut [u8],rx_buffer: &'rx_buf mut [u8],parser_frame_buffer: &'parser_frame_buffer mut [u8]) -> Self{
        let frame_parser = FrameParser::new(rx_buffer, parser_frame_buffer);

        Self {  tx_buffer, address: 0x01, pending_frame: None,frame_parser }
    }

    pub fn new_client(tx_buffer: &'tx_buf mut [u8],rx_buffer: &'rx_buf mut [u8],parser_frame_buffer: &'parser_frame_buffer mut [u8]) -> Self{

        let frame_parser = FrameParser::new(rx_buffer, parser_frame_buffer);
        Self { tx_buffer, address: 0x00, pending_frame: None,frame_parser }
    }

    pub fn read_tick<'b>(&'b mut self,new_data: &[u8]) -> Result<Option<Frame<'b>>,error::DecodeError>
    where 'parser_frame_buffer: 'b
     {
        self.frame_parser.write_data(new_data)?;
        let frame = match self.frame_parser.next_frame() {
                    Some(Ok(f)) => f,
                    Some(Err(e)) => return Err(e),
                    None => return Ok(None),
                };



        match frame.to_address() {
            Address::Unicast(a) => {
                if a != self.address {
                    return Ok(None)
                }
            },
            Address::Broadcast => (),
        }

        match frame.into_payload() {
            FramePayload::Ack =>{
                self.pending_frame = None;
            },
            FramePayload::SetAddress { address, id } => {
            if self.node_type == NodeType::Master{
                
            }
            }
            _ => {},
        };
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::FrameParser;

    #[test]
    fn test_encode_buffer_too_small() {
        let data: &[u8] = &[0x01, 0x02];
        let payload = FramePayload::Data(data);
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
        let frame = Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Data(data));

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
        assert_eq!(frame.payload(), &FramePayload::Data(expected_data));
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
        assert_eq!(frame.payload(), &FramePayload::Data(expected_data));
        assert!(parser.next_frame().is_none());
    }

    #[test]
    fn test_roundtrip_encode_decode_with_stuffing() {
        // 1. Arrange (準備)

        // エスケープ対象バイトをすべて含むペイロードを作成
        let test_data: &[u8] = &[0x01, SOF, 0x03, EOF, 0x05, ESC, 0x07];
        let original_frame = Frame::new(Address::Broadcast, 0x42, FramePayload::Data(test_data));

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
}
