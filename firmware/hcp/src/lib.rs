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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DeviceKind {
    UpperPanelDdi,
    ButtonPanel,
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
        assert!(!DisplayData { seq: 9, ..newer.clone() }.supersedes(10));
    }
}
