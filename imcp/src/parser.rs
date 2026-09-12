use crate::*;

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
    pub fn write_data(&mut self, new_data: &[u8]) -> Result<usize, DecodeError> {
        // 1. バッファを整理 (もし rx_scan_pos > 0 ならデータを詰める)
        self.consume_rx_buffer();

        // 2. 空き容量を計算
        let free_space = self.rx_buffer.len() - self.rx_len;
        if new_data.len() > free_space {
            return Err(DecodeError::FrameBufferTooSmall);
        }

        // 3. データをバッファの末尾にコピー
        self.rx_buffer[self.rx_len..self.rx_len + new_data.len()].copy_from_slice(new_data);
        self.rx_len += new_data.len();
        Ok(new_data.len())
    }

    /// rx_buffer を解析し、次の有効なフレームを返す
    /// (ライフタイム 'b は 'frame_buf に依存)
    pub fn next_frame<'b>(&'b mut self) -> Option<Result<Frame, DecodeError>>
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
                    if self.is_escaping {
                        // ESC の後に現れるのは、予約バイトを XOR した3種類だけ。
                        // それ以外を受け入れると、壊れたフレームを別の内容として
                        // 正常扱いしてしまう。
                        let is_valid_escape = byte == (SOF ^ ESC_XOR)
                            || byte == (EOF ^ ESC_XOR)
                            || byte == (ESC ^ ESC_XOR);
                        self.is_escaping = false;

                        if !is_valid_escape {
                            // SOF は現在の壊れたフレームを破棄し、次のフレームの
                            // 開始として再利用する。EOF/通常バイトの場合は、次の
                            // SOF まで待機する。
                            if byte == SOF {
                                self.state = ParserState::Receiving;
                                self.frame_len = 0;
                            } else {
                                self.reset();
                            }
                            return Some(Err(DecodeError::InvalidEscapeSequence));
                        }

                        if let Err(error) = self.push_frame_byte(byte ^ ESC_XOR) {
                            return Some(Err(error));
                        }
                        continue;
                    }

                    match byte {
                        SOF => {
                            // 予期せぬ SOF。破損したフレームを破棄し、ここから
                            // 新しいフレームとして再同期する。
                            self.frame_len = 0;
                        }
                        EOF => {
                            // EOF受信。フレーム終端
                            self.state = ParserState::WaitingForSof;

                            // frame_buffer (アンスタッフィング済み) をデコード
                            let decode_slice = &self.frame_buffer[..self.frame_len];

                            // 'b のライフタイムで返す
                            return Some(Frame::decode(decode_slice));
                        }
                        ESC => {
                            self.is_escaping = true;
                        }
                        _ => {
                            if let Err(error) = self.push_frame_byte(byte) {
                                return Some(Err(error));
                            }
                        }
                    }
                }
            }
        }

        // データ不足 (ループを抜けた)
        None
    }

    fn push_frame_byte(&mut self, byte: u8) -> Result<(), DecodeError> {
        if self.frame_len >= self.frame_buffer.len() {
            // アンスタッフィング後バッファが溢れた
            // フレームが長すぎる (破損)
            self.reset();
            return Err(DecodeError::FrameBufferTooSmall);
        }

        self.frame_buffer[self.frame_len] = byte;
        self.frame_len += 1;
        Ok(())
    }

    fn reset(&mut self) {
        self.state = ParserState::WaitingForSof;
        self.frame_len = 0;
        self.is_escaping = false;
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
