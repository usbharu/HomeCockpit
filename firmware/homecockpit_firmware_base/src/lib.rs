#![cfg_attr(not(test), no_std)]

use hcp::{
    APP_PROTOCOL_VERSION, AppPacketError, AppPacketKind, Capabilities, ControlEvent, ControlValue,
    DeviceHello, DeviceKind, DisplayData, Version, decode_data_packet, decode_set_packet,
    encode_set_packet,
};
use imcp::frame::{Address, Frame, FramePayload};

pub const IMCP_MASTER_ADDRESS: u8 = 0x01;
pub const FEATURE_CONTROL_EVENTS: u32 = 1 << 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FirmwareBaseError {
    Packet(AppPacketError),
    DeviceAddressUnassigned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DeviceRuntimeState {
    address: Option<u8>,
    next_control_seq: u16,
    last_display_seq: Option<u16>,
}

impl DeviceRuntimeState {
    pub const fn new() -> Self {
        Self {
            address: None,
            next_control_seq: 0,
            last_display_seq: None,
        }
    }

    pub fn address(&self) -> Option<u8> {
        self.address
    }

    pub fn assign_address(&mut self, address: u8) {
        self.address = Some(address);
        self.next_control_seq = 0;
        self.last_display_seq = None;
    }

    pub fn take_next_control_seq(&mut self) -> Result<u16, FirmwareBaseError> {
        if self.address.is_none() {
            return Err(FirmwareBaseError::DeviceAddressUnassigned);
        }

        let seq = self.next_control_seq;
        self.next_control_seq = self.next_control_seq.wrapping_add(1);
        Ok(seq)
    }

    pub fn accept_display_data(&mut self, display: &DisplayData) -> bool {
        let accepted = self
            .last_display_seq
            .is_none_or(|previous| display.supersedes(previous));
        if accepted {
            self.last_display_seq = Some(display.seq);
        }
        accepted
    }
}

impl Default for DeviceRuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DeviceDescriptor {
    pub device_id: u64,
    pub device_kind: DeviceKind,
    pub firmware_version: Version,
    pub capabilities: Capabilities,
}

impl DeviceDescriptor {
    pub fn protocol_version(&self) -> u8 {
        APP_PROTOCOL_VERSION
    }
}

pub fn control_id_from_matrix_position(row: u8, column: u8, columns: u8) -> u16 {
    u16::from(row) * u16::from(columns) + u16::from(column)
}

pub fn try_assign_address_from_frame(state: &mut DeviceRuntimeState, frame: &Frame) -> Option<u8> {
    match frame.payload() {
        FramePayload::SetAddress { address, .. } => {
            state.assign_address(*address);
            Some(*address)
        }
        _ => None,
    }
}

pub fn build_device_hello_packet(descriptor: DeviceDescriptor) -> AppPacketKind {
    AppPacketKind::DeviceHello(DeviceHello {
        device_id: descriptor.device_id,
        device_kind: descriptor.device_kind,
        protocol_version: descriptor.protocol_version(),
        firmware_version: descriptor.firmware_version,
        capabilities: descriptor.capabilities,
    })
}

pub fn build_button_control_event(
    state: &mut DeviceRuntimeState,
    control_id: u16,
    pressed: bool,
) -> Result<AppPacketKind, FirmwareBaseError> {
    let seq = state.take_next_control_seq()?;
    Ok(AppPacketKind::ControlEvent(ControlEvent {
        seq,
        control_id,
        event: ControlValue::Button { pressed },
    }))
}

pub fn encode_set_frame(
    from_address: u8,
    kind: &AppPacketKind,
) -> Result<Frame, FirmwareBaseError> {
    let payload = encode_set_packet(kind).map_err(FirmwareBaseError::Packet)?;
    Ok(Frame::new(
        Address::Unicast(IMCP_MASTER_ADDRESS),
        from_address,
        FramePayload::Set(payload),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct AppliedDisplay {
    pub data: DisplayData,
    pub accepted: bool,
}

/// Result of applying a display update or `RequestDeviceHello` from one frame.
///
/// Direct connections have no hub that rewrites the source address, so these
/// application commands apply only when the frame comes from the IMCP master.
/// IMCP itself still delivers `Data` and `Set` from other stations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct MasterApplicationEffect {
    pub display: Option<AppliedDisplay>,
    pub queue_device_hello: bool,
}

impl MasterApplicationEffect {
    pub const fn none() -> Self {
        Self {
            display: None,
            queue_device_hello: false,
        }
    }
}

pub fn apply_master_application_frame(
    state: &mut DeviceRuntimeState,
    frame: &Frame,
) -> MasterApplicationEffect {
    if frame.from_address() != IMCP_MASTER_ADDRESS {
        return MasterApplicationEffect::none();
    }

    let mut effect = MasterApplicationEffect::none();
    if let FramePayload::Set(payload) = frame.payload()
        && let Ok(AppPacketKind::ControlEvent(ControlEvent {
            control_id: hcp::CONTROL_ID_REQUEST_DEVICE_HELLO,
            event: ControlValue::RequestDeviceHello,
            ..
        })) = decode_set_packet(payload.as_slice())
        && state.address().is_some()
    {
        effect.queue_device_hello = true;
    }

    if let FramePayload::Data(payload) = frame.payload()
        && let Ok(data) = decode_data_packet(payload.as_slice())
    {
        let accepted = state.accept_display_data(&data);
        effect.display = Some(AppliedDisplay { data, accepted });
    }

    effect
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn device_runtime_state_resets_sequence_on_address_assignment() {
        let mut state = DeviceRuntimeState::new();
        state.assign_address(0x20);
        assert_eq!(state.take_next_control_seq().unwrap(), 0);
        assert_eq!(state.take_next_control_seq().unwrap(), 1);

        state.assign_address(0x21);
        assert_eq!(state.address(), Some(0x21));
        assert_eq!(state.take_next_control_seq().unwrap(), 0);
    }

    #[test]
    fn button_control_event_uses_runtime_sequence() {
        let mut state = DeviceRuntimeState::new();
        state.assign_address(0x02);

        let packet = build_button_control_event(&mut state, 7, true).unwrap();
        assert_eq!(
            packet,
            AppPacketKind::ControlEvent(ControlEvent {
                seq: 0,
                control_id: 7,
                event: ControlValue::Button { pressed: true },
            })
        );
    }

    #[test]
    fn display_sequence_accepts_newer_and_rejects_older_packets() {
        let mut state = DeviceRuntimeState::new();
        let newer = hcp::DisplayData {
            seq: 4,
            target: hcp::DisplayTarget::Screen(0),
            payload: hcp::DisplayPayload::Bytes {
                encoding: hcp::ByteEncoding::Utf8Text,
                data: Default::default(),
            },
        };
        let older = hcp::DisplayData {
            seq: 3,
            ..newer.clone()
        };

        assert!(state.accept_display_data(&newer));
        assert!(!state.accept_display_data(&older));
    }

    #[test]
    fn encode_set_frame_targets_master() {
        let frame = encode_set_frame(
            0x22,
            &build_device_hello_packet(DeviceDescriptor {
                device_id: 0x0123_4567_89AB_CDEF,
                device_kind: DeviceKind::ButtonPanel,
                firmware_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                capabilities: Capabilities {
                    displays: 1,
                    controls: 8,
                    features: FEATURE_CONTROL_EVENTS,
                },
            }),
        )
        .unwrap();

        assert_eq!(frame.to_address(), Address::Unicast(IMCP_MASTER_ADDRESS));
        assert_eq!(frame.from_address(), 0x22);
        assert!(matches!(frame.payload(), FramePayload::Set(_)));
    }

    fn display_frame(to: Address, from: u8, seq: u16) -> Frame {
        let display = hcp::DisplayData {
            seq,
            target: hcp::DisplayTarget::Screen(0),
            payload: hcp::DisplayPayload::Bytes {
                encoding: hcp::ByteEncoding::Utf8Text,
                data: Default::default(),
            },
        };
        let payload = hcp::encode_data_packet(&display).unwrap();
        Frame::new(to, from, FramePayload::Data(payload))
    }

    fn hello_request_frame(to: Address, from: u8, control_id: u16) -> Frame {
        let payload = encode_set_packet(&AppPacketKind::ControlEvent(ControlEvent {
            seq: 1,
            control_id,
            event: ControlValue::RequestDeviceHello,
        }))
        .unwrap();
        Frame::new(to, from, FramePayload::Set(payload))
    }

    #[test]
    fn non_master_frames_do_not_update_display_or_queue_hello() {
        let mut state = DeviceRuntimeState::new();
        state.assign_address(0x22);
        let assigned = Address::Unicast(0x22);

        assert_eq!(
            apply_master_application_frame(&mut state, &display_frame(assigned, 0x02, 4)),
            MasterApplicationEffect::none()
        );
        let accepted =
            apply_master_application_frame(&mut state, &display_frame(assigned, 0x01, 4));
        assert_eq!(
            accepted.display.as_ref().map(|display| display.accepted),
            Some(true)
        );
        assert!(!accepted.queue_device_hello);

        let repeated =
            apply_master_application_frame(&mut state, &display_frame(assigned, 0x01, 4));
        assert_eq!(
            repeated.display.as_ref().map(|display| display.accepted),
            Some(false)
        );

        assert_eq!(
            apply_master_application_frame(&mut state, &display_frame(Address::Broadcast, 0x02, 5)),
            MasterApplicationEffect::none()
        );
        let newer = apply_master_application_frame(
            &mut state,
            &display_frame(Address::Broadcast, IMCP_MASTER_ADDRESS, 5),
        );
        assert_eq!(
            newer.display.as_ref().map(|display| display.accepted),
            Some(true)
        );

        assert_eq!(
            apply_master_application_frame(
                &mut state,
                &hello_request_frame(assigned, 0x02, hcp::CONTROL_ID_REQUEST_DEVICE_HELLO)
            ),
            MasterApplicationEffect::none()
        );
        let hello = apply_master_application_frame(
            &mut state,
            &hello_request_frame(
                assigned,
                IMCP_MASTER_ADDRESS,
                hcp::CONTROL_ID_REQUEST_DEVICE_HELLO,
            ),
        );
        assert!(hello.queue_device_hello);
        assert!(hello.display.is_none());
        assert_eq!(
            apply_master_application_frame(
                &mut state,
                &hello_request_frame(assigned, 0x01, 0x0001)
            ),
            MasterApplicationEffect::none()
        );
    }

    #[test]
    fn request_device_hello_without_address_does_not_queue() {
        let mut state = DeviceRuntimeState::new();
        let effect = apply_master_application_frame(
            &mut state,
            &hello_request_frame(
                Address::Unicast(0x22),
                IMCP_MASTER_ADDRESS,
                hcp::CONTROL_ID_REQUEST_DEVICE_HELLO,
            ),
        );
        assert_eq!(effect, MasterApplicationEffect::none());
    }
}
