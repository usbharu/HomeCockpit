# homecockpit_firmware_base

HomeCockpit の各 firmware crate で共通利用するランタイム補助 crate です。

## 役割

- `hcp` を使った共通 packet 生成
- IMCP 上での送信先など、HomeCockpit firmware 共通の前提定義
- デバイスごとの runtime state 管理
  - 割り当て済み address
  - `ControlEvent` の sequence 採番

`hcp` は wire protocol 定義だけを持ちます。  
`homecockpit_firmware_base` はその上で使う実装補助を持ちます。

## 含めるもの

- `DeviceRuntimeState`
- `DeviceDescriptor`
- `build_device_hello_packet`
- `build_button_control_event`
- `encode_set_frame`
- `try_assign_address_from_frame`
- `control_id_from_matrix_position`

## 含めないもの

- デバイス固有の GPIO や matrix scan 実装
- 特定ボード専用の pin 配置
- 画面描画ロジック

## Example

```rust
use hcp::{Capabilities, DeviceKind, Version};
use homecockpit_firmware_base::{
    DeviceDescriptor, DeviceRuntimeState, FEATURE_CONTROL_EVENTS,
    build_button_control_event, build_device_hello_packet, encode_set_frame,
};

let descriptor = DeviceDescriptor {
    device_id: 0x5550_4449,
    device_kind: DeviceKind::UpperPanelDdi,
    firmware_version: Version { major: 0, minor: 1, patch: 0 },
    capabilities: Capabilities {
        displays: 0,
        controls: 20,
        features: FEATURE_CONTROL_EVENTS,
    },
};

let hello = build_device_hello_packet(descriptor);
let frame = encode_set_frame(0x02, &hello)?;

let mut state = DeviceRuntimeState::new();
state.assign_address(0x02);
let event = build_button_control_event(&mut state, 3, true)?;
let frame = encode_set_frame(0x02, &event)?;
```
