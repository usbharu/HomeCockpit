# HCP

`hcp` は HomeCockpit 向けのアプリケーション層プロトコル crate です。  
IMCP の `FramePayload::Data` と `FramePayload::Set` の中に載せる payload を定義します。

## Purpose

- `Data`: PC -> cockpit device の高頻度な表示更新
- `Set`: 双方向の確実配送メッセージ
  - device -> PC の自己紹介
  - device -> PC の操作イベント

HCP 自体は IMCP の下位 transport を持ちません。  
IMCP の ACK / retry をそのまま利用する前提です。

## Serialization

- binary codec: `postcard`
- type definition: `serde`
- max payload size: `128` bytes

1 HCP packet は 1 IMCP payload に収まる想定です。  
分割送信や再構成は HCP v1 では扱いません。

## Packet Model

すべての packet は次の envelope を持ちます。

```rust
pub struct AppPacket {
    pub version: u8,
    pub kind: AppPacketKind,
}
```

- `version`: HCP のバージョン。現在は `1`
- `kind`: 実際の payload 種別

### AppPacketKind

- `DisplayData`
- `DeviceHello`
- `ControlEvent`

運用ルール:

- `FramePayload::Data` には `DisplayData` だけを載せる
- `FramePayload::Set` には `DeviceHello` と `ControlEvent` を載せる

## Message Types

### DisplayData

表示更新用 packet です。

```rust
pub struct DisplayData {
    pub seq: u16,
    pub target: DisplayTarget,
    pub payload: DisplayPayload,
}
```

- `seq`: 単調増加のシーケンス番号
- `target`: 更新対象
- `payload`: 実際の表示内容

`DisplayData::supersedes(previous_seq)` で、古い表示更新を捨てる判定に使えます。

`DisplayTarget`

- `Screen(u8)`
- `Indicator(u16)`

`DisplayPayload`

- `Text { format, content }`
- `Bytes { encoding, data }`

`ByteEncoding`

- `MonoBitmap1bpp`
- `SegmentMap`
- `Utf8Text`

### DeviceHello

デバイスが接続後に自身の情報を通知する packet です。

```rust
pub struct DeviceHello {
    pub device_id: u64,
    pub device_kind: DeviceKind,
    pub protocol_version: u8,
    pub firmware_version: Version,
    pub capabilities: Capabilities,
}
```

`DeviceKind`

- `UpperPanelDdi`
- `ButtonPanel`
- `ImcpHub`

想定タイミング:

- IMCP の `Join`
- IMCP の `SetAddress`
- その後 `FramePayload::Set` で `DeviceHello`

`device_id` はデバイス固有の安定した ID です。  
`Join` に使う一時的な session ID とは別物として扱います。

### ControlEvent

デバイス操作を抽象化して通知する packet です。

```rust
pub struct ControlEvent {
    pub seq: u16,
    pub control_id: u16,
    pub event: ControlValue,
}
```

`ControlValue`

- `Button { pressed }`
- `EncoderDelta { steps }`
- `Absolute { value }`
- `Toggle { state }`
- `RequestDeviceHello`

`control_id` の意味は `device_kind` ごとに固定します。  
PC 側は `(device_kind, control_id)` で解釈してください。

プロトコル制御用の予約 control ID:

- `CONTROL_ID_REQUEST_DEVICE_HELLO = 0xFF00`

`ControlEvent { control_id: CONTROL_ID_REQUEST_DEVICE_HELLO, event: ControlValue::RequestDeviceHello }`
を受け取った側は、任意のタイミングで `DeviceHello` を再送できます。

## Public API

主要 API:

- `encode_data_packet(&DisplayData)`
- `encode_set_packet(&AppPacketKind)`
- `decode_app_packet(&[u8])`
- `decode_data_packet(&[u8])`
- `decode_set_packet(&[u8])`

エラーハンドリング:

- `BufferTooSmall`: 128 byte 制限を超えた
- `UnsupportedVersion`: 未対応の HCP version
- `InvalidDataPacketKind`: `Data` として不正な種別
- `InvalidSetPacketKind`: `Set` として不正な種別

## Example

```rust
use hcp::{
    AppPacketKind, Capabilities, ControlEvent, ControlValue, DeviceHello,
    DeviceKind, Version, CONTROL_ID_REQUEST_DEVICE_HELLO, encode_set_packet,
};

let hello = AppPacketKind::DeviceHello(DeviceHello {
    device_id: 0x0123_4567_89AB_CDEF,
    device_kind: DeviceKind::ImcpHub,
    protocol_version: 1,
    firmware_version: Version { major: 0, minor: 1, patch: 0 },
    capabilities: Capabilities {
        displays: 0,
        controls: 20,
        features: 1,
    },
});

let bytes = encode_set_packet(&hello)?;

let event = AppPacketKind::ControlEvent(ControlEvent {
    seq: 1,
    control_id: 3,
    event: ControlValue::Button { pressed: true },
});

let bytes = encode_set_packet(&event)?;

let request_hello = AppPacketKind::ControlEvent(ControlEvent {
    seq: 2,
    control_id: CONTROL_ID_REQUEST_DEVICE_HELLO,
    event: ControlValue::RequestDeviceHello,
});

let bytes = encode_set_packet(&request_hello)?;
```

## Compatibility Notes

- HCP v1 は Rust 同士の通信を前提にしている
- wire format は `postcard` 依存なので、他言語対応が必要なら別途仕様固定が必要
- IMCP 自体の frame type や ACK 挙動は HCP では変更しない
