#![cfg_attr(not(test), no_std)]

use heapless::{String, Vec};
use serde::{Deserialize, Serialize};

pub const APP_PROTOCOL_VERSION: u8 = 1;
pub const MAX_PAYLOAD_SIZE: usize = 128;
pub const MAX_TEXT_LEN: usize = MAX_PAYLOAD_SIZE;
pub const MAX_BINARY_LEN: usize = MAX_PAYLOAD_SIZE;
pub const CONTROL_ID_REQUEST_DEVICE_HELLO: u16 = 0xFF00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AppPacketError {
    BufferTooSmall,
    Serialize,
    Deserialize,
    UnsupportedVersion(u8),
    InvalidDataPacketKind,
    InvalidSetPacketKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct AppPacket {
    pub version: u8,
    pub kind: AppPacketKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AppPacketKind {
    DisplayData(DisplayData),
    DeviceHello(DeviceHello),
    ControlEvent(ControlEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DisplayData {
    pub seq: u16,
    pub target: DisplayTarget,
    pub payload: DisplayPayload,
}

impl DisplayData {
    pub fn supersedes(&self, previous_seq: u16) -> bool {
        self.seq != previous_seq && self.seq.wrapping_sub(previous_seq) < 0x8000
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DisplayTarget {
    Screen(u8),
    Indicator(u16),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DisplayPayload {
    Text {
        format: TextFormat,
        content: String<MAX_TEXT_LEN>,
    },
    Bytes {
        encoding: ByteEncoding,
        data: Vec<u8, MAX_BINARY_LEN>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum TextFormat {
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ByteEncoding {
    MonoBitmap1bpp,
    SegmentMap,
    Utf8Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DeviceHello {
    pub device_id: u64,
    pub device_kind: DeviceKind,
    pub protocol_version: u8,
    pub firmware_version: Version,
    pub capabilities: Capabilities,
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DeviceKind {
    UpperPanelDdi = 0,
    ButtonPanel = 1,
    ImcpHub = 2,
    Unknown(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Version {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Capabilities {
    pub displays: u8,
    pub controls: u16,
    pub features: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ControlEvent {
    pub seq: u16,
    pub control_id: u16,
    pub event: ControlValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ControlValue {
    Button { pressed: bool },
    EncoderDelta { steps: i8 },
    Absolute { value: i16 },
    Toggle { state: bool },
    RequestDeviceHello,
}

pub fn encode_data_packet(
    display: &DisplayData,
) -> Result<Vec<u8, MAX_PAYLOAD_SIZE>, AppPacketError> {
    encode_packet(&AppPacket {
        version: APP_PROTOCOL_VERSION,
        kind: AppPacketKind::DisplayData(display.clone()),
    })
}

pub fn encode_set_packet(
    kind: &AppPacketKind,
) -> Result<Vec<u8, MAX_PAYLOAD_SIZE>, AppPacketError> {
    if matches!(kind, AppPacketKind::DisplayData(_)) {
        return Err(AppPacketError::InvalidSetPacketKind);
    }

    encode_packet(&AppPacket {
        version: APP_PROTOCOL_VERSION,
        kind: kind.clone(),
    })
}

pub fn decode_app_packet(bytes: &[u8]) -> Result<AppPacket, AppPacketError> {
    let packet: AppPacket = postcard::from_bytes(bytes).map_err(map_postcard_decode_error)?;
    if packet.version != APP_PROTOCOL_VERSION {
        return Err(AppPacketError::UnsupportedVersion(packet.version));
    }
    Ok(packet)
}

pub fn decode_data_packet(bytes: &[u8]) -> Result<DisplayData, AppPacketError> {
    match decode_app_packet(bytes)?.kind {
        AppPacketKind::DisplayData(data) => Ok(data),
        _ => Err(AppPacketError::InvalidDataPacketKind),
    }
}

pub fn decode_set_packet(bytes: &[u8]) -> Result<AppPacketKind, AppPacketError> {
    let packet = decode_app_packet(bytes)?;
    if matches!(packet.kind, AppPacketKind::DisplayData(_)) {
        return Err(AppPacketError::InvalidSetPacketKind);
    }
    Ok(packet.kind)
}

fn encode_packet(packet: &AppPacket) -> Result<Vec<u8, MAX_PAYLOAD_SIZE>, AppPacketError> {
    let mut buffer = [0u8; MAX_PAYLOAD_SIZE];
    let encoded = postcard::to_slice(packet, &mut buffer).map_err(map_postcard_encode_error)?;
    Vec::from_slice(encoded).map_err(|_| AppPacketError::BufferTooSmall)
}

fn map_postcard_encode_error(error: postcard::Error) -> AppPacketError {
    match error {
        postcard::Error::SerializeBufferFull => AppPacketError::BufferTooSmall,
        _ => AppPacketError::Serialize,
    }
}

fn map_postcard_decode_error(_error: postcard::Error) -> AppPacketError {
    AppPacketError::Deserialize
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn display_data_roundtrip_works() {
        let payload = DisplayData {
            seq: 42,
            target: DisplayTarget::Screen(1),
            payload: DisplayPayload::Bytes {
                encoding: ByteEncoding::MonoBitmap1bpp,
                data: Vec::from_slice(&[0xAA, 0x55, 0xF0]).unwrap(),
            },
        };

        let encoded = encode_data_packet(&payload).unwrap();
        let decoded = decode_data_packet(&encoded).unwrap();

        assert_eq!(decoded, payload);
    }

    #[test]
    fn set_device_hello_roundtrip_works() {
        let packet = AppPacketKind::DeviceHello(DeviceHello {
            device_id: 0x0123_4567_89AB_CDEF,
            device_kind: DeviceKind::UpperPanelDdi,
            protocol_version: APP_PROTOCOL_VERSION,
            firmware_version: Version {
                major: 0,
                minor: 1,
                patch: 0,
            },
            capabilities: Capabilities {
                displays: 2,
                controls: 20,
                features: 0x03,
            },
        });

        let encoded = encode_set_packet(&packet).unwrap();
        let decoded = decode_set_packet(&encoded).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn set_imcp_hub_device_hello_roundtrip_works() {
        let packet = AppPacketKind::DeviceHello(DeviceHello {
            device_id: 0x0BAD_F00D_CAFE_BEEF,
            device_kind: DeviceKind::ImcpHub,
            protocol_version: APP_PROTOCOL_VERSION,
            firmware_version: Version {
                major: 1,
                minor: 2,
                patch: 3,
            },
            capabilities: Capabilities {
                displays: 0,
                controls: 1,
                features: 0x01,
            },
        });

        let encoded = encode_set_packet(&packet).unwrap();
        let decoded = decode_set_packet(&encoded).unwrap();

        assert_eq!(decoded, packet);
    }

    #[test]
    fn set_control_event_roundtrip_works() {
        let button = AppPacketKind::ControlEvent(ControlEvent {
            seq: 7,
            control_id: 12,
            event: ControlValue::Button { pressed: true },
        });
        let encoded = encode_set_packet(&button).unwrap();
        let decoded = decode_set_packet(&encoded).unwrap();
        assert_eq!(decoded, button);

        let encoder = AppPacketKind::ControlEvent(ControlEvent {
            seq: 8,
            control_id: 14,
            event: ControlValue::EncoderDelta { steps: -2 },
        });
        let encoded = encode_set_packet(&encoder).unwrap();
        let decoded = decode_set_packet(&encoded).unwrap();
        assert_eq!(decoded, encoder);

        let request = AppPacketKind::ControlEvent(ControlEvent {
            seq: 9,
            control_id: CONTROL_ID_REQUEST_DEVICE_HELLO,
            event: ControlValue::RequestDeviceHello,
        });
        let encoded = encode_set_packet(&request).unwrap();
        let decoded = decode_set_packet(&encoded).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn oversized_payload_is_rejected() {
        let data = Vec::from_slice(&[0xAB; MAX_BINARY_LEN]).unwrap();
        let packet = DisplayData {
            seq: 1,
            target: DisplayTarget::Screen(0),
            payload: DisplayPayload::Bytes {
                encoding: ByteEncoding::MonoBitmap1bpp,
                data,
            },
        };

        let result = encode_data_packet(&packet);
        assert_eq!(result, Err(AppPacketError::BufferTooSmall));
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let packet = AppPacket {
            version: APP_PROTOCOL_VERSION.saturating_add(1),
            kind: AppPacketKind::ControlEvent(ControlEvent {
                seq: 1,
                control_id: 1,
                event: ControlValue::Toggle { state: true },
            }),
        };

        let mut buffer = [0u8; MAX_PAYLOAD_SIZE];
        let encoded = postcard::to_slice(&packet, &mut buffer).unwrap();
        let result = decode_app_packet(encoded);

        assert_eq!(
            result,
            Err(AppPacketError::UnsupportedVersion(
                APP_PROTOCOL_VERSION.saturating_add(1)
            ))
        );
    }

    #[test]
    fn set_packet_cannot_encode_display_data() {
        let result = encode_set_packet(&AppPacketKind::DisplayData(DisplayData {
            seq: 1,
            target: DisplayTarget::Screen(0),
            payload: DisplayPayload::Bytes {
                encoding: ByteEncoding::Utf8Text,
                data: Vec::new(),
            },
        }));

        assert_eq!(result, Err(AppPacketError::InvalidSetPacketKind));
    }

    #[test]
    fn display_sequence_can_reject_older_packets() {
        let newer = DisplayData {
            seq: 10,
            target: DisplayTarget::Screen(0),
            payload: DisplayPayload::Bytes {
                encoding: ByteEncoding::SegmentMap,
                data: Vec::new(),
            },
        };

        assert!(newer.supersedes(9));
        assert!(!newer.supersedes(10));
        assert!(
            !DisplayData {
                seq: 9,
                ..newer.clone()
            }
            .supersedes(10)
        );
    }
}

#[cfg(any(test, kani))]
fn display_at(seq: u16) -> DisplayData {
    DisplayData {
        seq,
        target: DisplayTarget::Screen(0),
        payload: DisplayPayload::Bytes {
            encoding: ByteEncoding::Utf8Text,
            data: Vec::new(),
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod properties {
    use super::*;
    use proptest::prelude::*;

    fn arb_text() -> impl Strategy<Value = String<MAX_TEXT_LEN>> {
        prop::collection::vec(0x20u8..0x7F, 0..=32).prop_map(|bytes| {
            let text = core::str::from_utf8(&bytes).expect("ascii is utf-8");
            String::try_from(text).expect("text fits")
        })
    }

    fn arb_bytes() -> impl Strategy<Value = Vec<u8, MAX_BINARY_LEN>> {
        prop::collection::vec(any::<u8>(), 0..=32)
            .prop_map(|bytes| Vec::from_slice(&bytes).expect("bytes fit"))
    }

    fn arb_display() -> impl Strategy<Value = DisplayData> {
        (
            any::<u16>(),
            any::<bool>(),
            any::<u8>(),
            any::<u16>(),
            any::<bool>(),
            arb_text(),
            arb_bytes(),
        )
            .prop_map(|(seq, screen, screen_id, indicator, text, content, data)| {
                DisplayData {
                    seq,
                    target: if screen {
                        DisplayTarget::Screen(screen_id)
                    } else {
                        DisplayTarget::Indicator(indicator)
                    },
                    payload: if text {
                        DisplayPayload::Text {
                            format: TextFormat::Plain,
                            content,
                        }
                    } else {
                        DisplayPayload::Bytes {
                            encoding: ByteEncoding::MonoBitmap1bpp,
                            data,
                        }
                    },
                }
            })
    }

    fn arb_control_value() -> impl Strategy<Value = ControlValue> {
        prop_oneof![
            any::<bool>().prop_map(|pressed| ControlValue::Button { pressed }),
            any::<i8>().prop_map(|steps| ControlValue::EncoderDelta { steps }),
            any::<i16>().prop_map(|value| ControlValue::Absolute { value }),
            any::<bool>().prop_map(|state| ControlValue::Toggle { state }),
            Just(ControlValue::RequestDeviceHello),
        ]
    }

    fn arb_device_kind() -> impl Strategy<Value = DeviceKind> {
        prop_oneof![
            Just(DeviceKind::UpperPanelDdi),
            Just(DeviceKind::ButtonPanel),
            Just(DeviceKind::ImcpHub),
            any::<u16>().prop_map(DeviceKind::Unknown),
        ]
    }

    fn arb_set_kind() -> impl Strategy<Value = AppPacketKind> {
        prop_oneof![
            (
                any::<u64>(),
                arb_device_kind(),
                any::<u8>(),
                any::<u8>(),
                any::<u8>(),
                any::<u8>(),
                any::<u16>(),
                any::<u32>(),
            )
                .prop_map(
                    |(
                        device_id,
                        device_kind,
                        protocol_version,
                        major,
                        minor,
                        patch,
                        controls,
                        features,
                    )| {
                        AppPacketKind::DeviceHello(DeviceHello {
                            device_id,
                            device_kind,
                            protocol_version,
                            firmware_version: Version {
                                major,
                                minor,
                                patch,
                            },
                            capabilities: Capabilities {
                                displays: 0,
                                controls,
                                features,
                            },
                        })
                    },
                ),
            (any::<u16>(), any::<u16>(), arb_control_value()).prop_map(
                |(seq, control_id, event)| {
                    AppPacketKind::ControlEvent(ControlEvent {
                        seq,
                        control_id,
                        event,
                    })
                }
            ),
        ]
    }

    proptest! {
        #[test]
        fn display_and_set_packets_roundtrip_within_the_size_limit(
            display in arb_display(),
            set_kind in arb_set_kind(),
        ) {
            let encoded = encode_data_packet(&display).unwrap();
            prop_assert!(encoded.len() <= MAX_PAYLOAD_SIZE);
            prop_assert_eq!(decode_data_packet(&encoded).unwrap(), display);
            prop_assert!(decode_set_packet(&encoded).is_err());

            let encoded = encode_set_packet(&set_kind).unwrap();
            prop_assert!(encoded.len() <= MAX_PAYLOAD_SIZE);
            prop_assert_eq!(decode_set_packet(&encoded).unwrap(), set_kind);
            prop_assert!(decode_data_packet(&encoded).is_err());
        }

        #[test]
        fn set_packets_reject_display_data(display in arb_display()) {
            let result = encode_set_packet(&AppPacketKind::DisplayData(display));
            prop_assert!(result.is_err());
        }

        #[test]
        fn supersedes_is_antisymmetric_except_on_the_half_turn(
            seq in any::<u16>(),
            previous in any::<u16>(),
        ) {
            let forward = display_at(seq).supersedes(previous);
            let backward = display_at(previous).supersedes(seq);
            if seq == previous || seq.wrapping_sub(previous) == 0x8000 {
                prop_assert!(!forward);
                prop_assert!(!backward);
            } else {
                prop_assert!(forward != backward);
            }
        }
    }

    #[test]
    fn supersedes_rejects_the_half_circle_boundary() {
        assert!(!display_at(0).supersedes(0));
        assert!(!display_at(0x8000).supersedes(0));
        assert!(!display_at(0).supersedes(0x8000));
        assert!(!display_at(1).supersedes(1u16.wrapping_add(0x8000)));
        assert!(display_at(1).supersedes(0));
        assert!(!display_at(0).supersedes(1));
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn supersedes_partitions_the_sequence_space() {
        let seq: u16 = kani::any();
        let previous: u16 = kani::any();
        let forward = display_at(seq).supersedes(previous);
        let backward = display_at(previous).supersedes(seq);

        if seq == previous {
            assert!(!forward);
            assert!(!backward);
        } else if seq.wrapping_sub(previous) == 0x8000 {
            assert!(!forward);
            assert!(!backward);
        } else {
            assert!(forward != backward);
            assert!(forward || backward);
        }
    }
}
