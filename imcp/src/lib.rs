#![cfg_attr(not(test), no_std)]

pub const SOF: u8 = 0xFE;
pub const EOF: u8 = 0xFF;
pub const ESC: u8 = 0xFD;

const ESC_XOR: u8 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Address {
    Unicast(u8),
    Broadcast,
}

impl Address {
    pub fn as_byte(&self) -> u8 {
        match self {
            Address::Unicast(addr) => *addr,
            Address::Broadcast => 0xFF,
        }
    }

    pub fn from_byte(byte: u8) -> Self {
        if byte == 0xFF {
            Address::Broadcast
        } else {
            Address::Unicast(byte)
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Ping = 0,
    Pong = 1,
    Ack = 2,
    Join = 3,
    SetAddress = 4,
    Data = 5,
    Set = 6,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramePayload<'a> {
    Ping,
    Pong,
    Ack,
    Join,
    SetAddress(u8),
    Data(&'a [u8]),
    Set,
}

impl FrameType {
    /// u8 から FrameType への変換
    /// 不明なタイプの場合はエラーを返す
    fn from_byte(byte: u8) -> Result<Self, DecodeError> {
        match byte {
            0 => Ok(FrameType::Ping),
            1 => Ok(FrameType::Pong),
            2 => Ok(FrameType::Ack),
            3 => Ok(FrameType::Join),
            4 => Ok(FrameType::SetAddress),
            5 => Ok(FrameType::Data),
            6 => Ok(FrameType::Set),
            // 一致しない場合は UnknownFrameType エラー
            _ => Err(DecodeError::UnknownFrameType(byte)),
        }
    }
}

impl<'a> FramePayload<'a> {
    pub fn frame_type(&self) -> FrameType {
        match self {
            FramePayload::Ping => FrameType::Ping,
            FramePayload::Pong => FrameType::Pong,
            FramePayload::Ack => FrameType::Ack,
            FramePayload::Join => FrameType::Join,
            FramePayload::SetAddress(_) => FrameType::SetAddress,
            FramePayload::Data(_) => FrameType::Data,
            FramePayload::Set => FrameType::Set,
        }
    }
    pub fn len(&self) -> u16 {
        match self {
            FramePayload::Ping
            | FramePayload::Pong
            | FramePayload::Ack
            | FramePayload::Join
            | FramePayload::Set => 0,

            FramePayload::SetAddress(_) => 1,

            FramePayload::Data(data) => data.len() as u16,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// バイトスライスとフレームタイプからペイロードをデコードする
    ///
    /// # 引数
    /// * `frame_type` - ヘッダーから読み取ったフレームタイプ
    /// * `payload_slice` - ペイロード部分のみを切り出したスライス
    ///
    /// # 戻り値
    /// * `Ok(FramePayload)` - デコードされたペイロード
    /// * `Err(CorruptionError)` - ペイロード長がタイプと矛盾する場合
    fn decode(frame_type: FrameType, payload_slice: &'a [u8]) -> Result<Self, DecodeError> {
        let payload_len = payload_slice.len();

        match frame_type {
            // --- ペイロード長が 0 であるべきタイプ ---
            FrameType::Ping => {
                if payload_len != 0 {
                    return Err(DecodeError::InvalidPayloadLength);
                }
                Ok(FramePayload::Ping)
            }
            FrameType::Pong => {
                if payload_len != 0 {
                    return Err(DecodeError::InvalidPayloadLength);
                }
                Ok(FramePayload::Pong)
            }
            FrameType::Ack => {
                if payload_len != 0 {
                    return Err(DecodeError::InvalidPayloadLength);
                }
                Ok(FramePayload::Ack)
            }
            FrameType::Join => {
                if payload_len != 0 {
                    return Err(DecodeError::InvalidPayloadLength);
                }
                Ok(FramePayload::Join)
            }
            FrameType::Set => {
                if payload_len != 0 {
                    return Err(DecodeError::InvalidPayloadLength);
                }
                Ok(FramePayload::Set)
            }

            // --- ペイロード長が 1 であるべきタイプ ---
            FrameType::SetAddress => {
                if payload_len != 1 {
                    return Err(DecodeError::InvalidPayloadLength);
                }
                Ok(FramePayload::SetAddress(payload_slice[0]))
            }

            // --- 任意のペイロード長を許可するタイプ ---
            FrameType::Data => {
                // 任意の長さ (0 も含む) を許可
                Ok(FramePayload::Data(payload_slice))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame<'a> {
    to_address: Address,
    from_address: u8,
    payload: FramePayload<'a>,
}

impl<'a> Frame<'a> {
    pub fn new(to: Address, from: u8, payload: FramePayload<'a>) -> Self {
        Self {
            to_address: to,
            from_address: from,
            payload,
        }
    }

    pub fn to_address(&self) -> Address {
        self.to_address
    }

    pub fn from_address(&self) -> u8 {
        self.from_address
    }

    pub fn payload(&self) -> &FramePayload<'a> {
        &self.payload
    }

    pub fn payload_mut(&mut self) -> &mut FramePayload<'a> {
        &mut self.payload
    }

    pub fn into_payload(self) -> FramePayload<'a> {
        self.payload
    }

    pub const HEADER_LEN: usize = 1 + 1 + 1 + 2; // 5 bytes
    /// チェックサムの長さ
    pub const CHECKSUM_LEN: usize = 1;

    /// (ヘッダー + ペイロード + チェックサム)
    pub fn encoded_len(&self) -> usize {
        Self::HEADER_LEN + self.payload.len() as usize + Self::CHECKSUM_LEN
    }

    fn calculate_xor_checksum(data: &[u8]) -> u8 {
        data.iter().fold(0, |acc, &byte| acc ^ byte)
    }

    pub fn encode(&self, buffer: &mut [u8]) -> Result<usize, EncodeError> {
        let payload_len = self.payload.len();
        let mut checksum: u8 = 0;
        let mut write_idx: usize = 0;

        // バッファに1バイト書き込む内部関数
        let write_raw_byte = |byte: u8, idx: usize, buf: &mut [u8]| -> Result<usize, EncodeError> {
            if idx >= buf.len() {
                return Err(EncodeError::BufferTooSmall);
            }
            buf[idx] = byte;
            Ok(idx + 1)
        };

        // データ部 (H+P+C) の1バイトをスタッフィングして書き込む内部関数
        let write_stuffed_byte =
            |byte: u8, idx: usize, buf: &mut [u8]| -> Result<usize, EncodeError> {
                match byte {
                    SOF | EOF | ESC => {
                        let idx = write_raw_byte(ESC, idx, buf)?;
                        write_raw_byte(byte ^ ESC_XOR, idx, buf)
                    }
                    _ => write_raw_byte(byte, idx, buf),
                }
            };

        // 1. SOF (スタッフィング対象外)
        write_idx = write_raw_byte(SOF, write_idx, buffer)?;

        // 2. Header (H)
        let mut header_bytes = [0u8; Self::HEADER_LEN];
        header_bytes[0] = self.to_address.as_byte();
        header_bytes[1] = self.from_address;
        header_bytes[2] = self.payload.frame_type() as u8;
        let len_bytes = payload_len.to_le_bytes();
        header_bytes[3] = len_bytes[0];
        header_bytes[4] = len_bytes[1];

        for &byte in &header_bytes {
            checksum ^= byte;
            write_idx = write_stuffed_byte(byte, write_idx, buffer)?;
        }

        // 3. Payload (P)
        if payload_len > 0 {
            // (SetAddress のペイロードもスライスとして扱えるように修正)
            let payload_data: &[u8] = match &self.payload {
                FramePayload::SetAddress(addr) => core::slice::from_ref(addr),
                FramePayload::Data(data) => data,
                _ => &[],
            };
            for &byte in payload_data {
                checksum ^= byte;
                write_idx = write_stuffed_byte(byte, write_idx, buffer)?;
            }
        }

        // 4. Checksum (C)
        write_idx = write_stuffed_byte(checksum, write_idx, buffer)?;

        // 5. EOF (スタッフィング対象外)
        write_idx = write_raw_byte(EOF, write_idx, buffer)?;

        Ok(write_idx)
    }

    /// バイトスライス（スタッフィング解除済み）からフレームをデコードする
    /// (戻り値から消費バイト数 usize を削除)
    pub fn decode(buffer: &'a [u8]) -> Result<Frame<'a>, DecodeError> {
        // 1. 最小長チェック (Header + Checksum)
        let min_len = Self::HEADER_LEN + Self::CHECKSUM_LEN;
        if buffer.len() < min_len {
            // EOFを受け取ったのに純粋なフレームが短すぎる = 破損
            return Err(DecodeError::InvalidPayloadLength);
        }

        // 2. チェックサム検証
        let data_len = buffer.len() - Self::CHECKSUM_LEN;
        let data_slice = &buffer[..data_len];
        let checksum_byte = buffer[data_len];

        let expected_checksum = Self::calculate_xor_checksum(data_slice);

        if checksum_byte != expected_checksum {
            return Err(DecodeError::InvalidChecksum);
        }

        // 3. ヘッダーフィールドの抽出
        let to_address = Address::from_byte(buffer[0]);
        let from_address = buffer[1];
        let frame_type_byte = buffer[2];
        let payload_len = u16::from_le_bytes([buffer[3], buffer[4]]);

        // 4. ヘッダーのペイロード長と実際のペイロード長が一致するか検証
        let actual_payload_len = data_len - Self::HEADER_LEN;
        if (payload_len as usize) != actual_payload_len {
            return Err(DecodeError::InvalidPayloadLength);
        }

        // 5. フレームタイプを解析
        let frame_type = FrameType::from_byte(frame_type_byte)?;

        // 6. ペイロードを解析
        let payload_slice = &data_slice[Self::HEADER_LEN..];
        let payload = FramePayload::decode(frame_type, payload_slice)?;

        // 7. フレームを構築
        Ok(Frame {
            to_address,
            from_address,
            payload,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// 書き込み先バッファのサイズが不足している
    BufferTooSmall,
}

/// デコード時に発生する可能性のあるエラー
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    InvalidChecksum,
    /// 未定義のフレームタイプID
    UnknownFrameType(u8),
    /// ペイロード長がフレームタイプや実際のデータ長と矛盾
    InvalidPayloadLength,
    /// スタッフィング解除後バッファが不足
    FrameBufferTooSmall,
    /// エスケープシーケンスが不正 (例: ESC の直後に EOF/SOF が来た)
    InvalidEscapeSequence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    /// SOF (0xFE) を探している
    WaitingForSof,
    /// SOF を受信し、データ (と EOF) を待っている
    Receiving,
}

impl<'rx_buf, 'frame_buf> FrameParser<'rx_buf, 'frame_buf> {
    /// 新しいパーサーを作成する
    ///
    /// # 引数
    /// * `rx_buffer` - UARTなどからの生データを蓄積するバッファ
    /// * `frame_buffer` - スタッフィング解除後のフレームを格納するバッファ
    pub fn new(rx_buffer: &'rx_buf mut [u8], frame_buffer: &'frame_buf mut [u8]) -> Self {
        Self {
            rx_buffer,
            rx_len: 0,
            rx_scan_pos: 0,
            frame_buffer,
            frame_len: 0,
            state: ParserState::WaitingForSof,
            is_escaping: false,
        }
    }

    /// rx_buffer の末尾に新しいデータを追加（書き込み）する
    pub fn write_data(&mut self, new_data: &[u8]) -> Result<usize, EncodeError> {
        // 1. バッファを整理 (もし rx_scan_pos > 0 ならデータを詰める)
        self.consume_rx_buffer();

        // 2. 空き容量を計算
        let free_space = self.rx_buffer.len() - self.rx_len;
        if new_data.len() > free_space {
            return Err(EncodeError::BufferTooSmall);
        }

        // 3. データをバッファの末尾にコピー
        self.rx_buffer[self.rx_len..self.rx_len + new_data.len()].copy_from_slice(new_data);
        self.rx_len += new_data.len();
        Ok(new_data.len())
    }

    /// rx_buffer を解析し、次の有効なフレームを返す
    /// (ライフタイム 'b は 'frame_buf に依存)
    pub fn next_frame<'b>(&'b mut self) -> Option<Result<Frame<'b>, DecodeError>>
    where
        'frame_buf: 'b,
    {
        while self.rx_scan_pos < self.rx_len {
            let byte = self.rx_buffer[self.rx_scan_pos];
            self.rx_scan_pos += 1; // バイトを消費

            match self.state {
                ParserState::WaitingForSof => {
                    if byte == SOF {
                        // SOF受信。受信モードへ移行
                        self.state = ParserState::Receiving;
                        self.frame_len = 0;
                        self.is_escaping = false;
                    }
                    // SOF以外は無視 (同期ズレ)
                }

                ParserState::Receiving => {
                    match byte {
                        SOF => {
                            // 予期せぬ SOF。フレームの再開とみなす
                            self.frame_len = 0;
                            self.is_escaping = false;
                            // (継続)
                        }
                        EOF => {
                            // EOF受信。フレーム終端
                            self.state = ParserState::WaitingForSof;
                            if self.is_escaping {
                                // ESC + EOF は不正
                                self.is_escaping = false;
                                return Some(Err(DecodeError::InvalidEscapeSequence));
                            }

                            // frame_buffer (アンスタッフィング済み) をデコード
                            let decode_slice = &self.frame_buffer[..self.frame_len];

                            // 'b のライフタイムで返す
                            return Some(Frame::decode(decode_slice));
                        }
                        ESC => {
                            if self.is_escaping {
                                // ESC + ESC は不正
                                self.state = ParserState::WaitingForSof;
                                return Some(Err(DecodeError::InvalidEscapeSequence));
                            }
                            self.is_escaping = true;
                        }
                        _ => {
                            // 通常データ or エスケープ解除データ
                            if self.frame_len >= self.frame_buffer.len() {
                                // アンスタッフィング後バッファが溢れた
                                // フレームが長すぎる (破損)
                                self.state = ParserState::WaitingForSof;
                                return Some(Err(DecodeError::FrameBufferTooSmall));
                            }

                            if self.is_escaping {
                                // エスケープ解除
                                self.frame_buffer[self.frame_len] = byte ^ ESC_XOR;
                                self.is_escaping = false;
                            } else {
                                // 通常データ
                                self.frame_buffer[self.frame_len] = byte;
                            }
                            self.frame_len += 1;
                        }
                    }
                }
            }
        }

        // データ不足 (ループを抜けた)
        None
    }

    /// rx_buffer の消費済み領域 (0..rx_scan_pos) を破棄し、
    /// 有効なデータ (rx_scan_pos..rx_len) をバッファの先頭に移動する
    pub fn consume_rx_buffer(&mut self) {
        if self.rx_scan_pos > 0 {
            self.rx_buffer.copy_within(self.rx_scan_pos..self.rx_len, 0);
            self.rx_len -= self.rx_scan_pos;
            self.rx_scan_pos = 0;
        }
    }
}

/// ストリーミングデコード用のパーサー (SOF/EOF/スタッフィング対応)
///
/// 2つのバッファ (生受信用, フレーム解析用) を使って動作します
pub struct FrameParser<'rx_buf, 'frame_buf> {
    // --- 生データ (スタッフィング済み) 用 ---
    rx_buffer: &'rx_buf mut [u8],
    rx_len: usize,
    /// rx_buffer 内の、次に解析を開始する位置
    rx_scan_pos: usize,

    // --- 解析データ (アンスタッフィング後) 用 ---
    frame_buffer: &'frame_buf mut [u8],
    /// frame_buffer 内の有効なデータ長
    frame_len: usize,

    state: ParserState,
    /// ESC (0xFD) を受信した直後か
    is_escaping: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checksum_calculation() {
        assert_eq!(Frame::calculate_xor_checksum(&[0x01, 0x02, 0x03]), 0x00);
        assert_eq!(Frame::calculate_xor_checksum(&[0xFF, 0x01]), 0xFE);
        assert_eq!(Frame::calculate_xor_checksum(&[]), 0x00);
    }

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
