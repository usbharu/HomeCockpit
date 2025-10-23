use crate::frame::FrameType;

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
pub enum ProtocolError{
    InvalidFrameType(FrameType)
}

#[derive(Debug,Copy,Clone,PartialEq,Eq)]
pub enum ImcpError {
    ProtocolError(ProtocolError),
    DecodeError(DecodeError),
    EncodeError(EncodeError)
}