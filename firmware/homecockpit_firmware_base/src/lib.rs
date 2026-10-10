#![cfg_attr(not(test), no_std)]

use hcp::{
    APP_PROTOCOL_VERSION, AppPacketError, AppPacketKind, Capabilities, ControlEvent, ControlValue,
    DeviceHello, DeviceKind, DisplayData, Version, encode_set_packet,
};
use imcp::frame::{Address, Frame, FramePayload};

pub const IMCP_MASTER_ADDRESS: u8 = 0x01;
// `1 << 0` and `1 >> 0` are both 1. That operator mutant is excluded in
// `.cargo/mutants.toml`.
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
        // Returning the literal 1 is equivalent while APP_PROTOCOL_VERSION is 1.
        // That replacement is excluded in .cargo/mutants.toml.
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod properties {
    use super::*;
    use proptest::prelude::*;

    fn display(seq: u16) -> hcp::DisplayData {
        hcp::DisplayData {
            seq,
            target: hcp::DisplayTarget::Screen(0),
            payload: hcp::DisplayPayload::Bytes {
                encoding: hcp::ByteEncoding::Utf8Text,
                data: Default::default(),
            },
        }
    }

    proptest! {
        #[test]
        fn control_sequence_restarts_when_the_address_is_assigned(
            first_steps in 0u16..64,
            second_steps in 0u16..64,
            first_address in 0x02u8..=0xFE,
            second_address in 0x02u8..=0xFE,
        ) {
            let mut state = DeviceRuntimeState::new();
            prop_assert!(state.take_next_control_seq().is_err());

            state.assign_address(first_address);
            prop_assert_eq!(state.address(), Some(first_address));
            for expected in 0..first_steps {
                prop_assert_eq!(state.take_next_control_seq().unwrap(), expected);
            }

            state.assign_address(second_address);
            prop_assert_eq!(state.address(), Some(second_address));
            for expected in 0..second_steps {
                prop_assert_eq!(state.take_next_control_seq().unwrap(), expected);
            }
        }

        #[test]
        fn accepted_display_updates_match_sequence_supersedes(
            first in any::<u16>(),
            later in prop::collection::vec(any::<u16>(), 0..8),
        ) {
            let mut state = DeviceRuntimeState::new();
            let mut last = None;
            for seq in std::iter::once(first).chain(later) {
                let packet = display(seq);
                let expected = last.is_none_or(|previous| packet.supersedes(previous));
                prop_assert_eq!(state.accept_display_data(&packet), expected);
                if expected {
                    last = Some(seq);
                }
            }
        }

        #[test]
        fn control_ids_are_row_major(
            row in 0u8..=40,
            column in 0u8..=40,
            columns in 1u8..=40,
        ) {
            prop_assume!(u16::from(column) < u16::from(columns));
            let expected = u32::from(row) * u32::from(columns) + u32::from(column);
            prop_assume!(expected <= u32::from(u16::MAX));
            prop_assert_eq!(
                u32::from(control_id_from_matrix_position(row, column, columns)),
                expected
            );
        }

        #[test]
        fn only_set_address_frames_assign_a_runtime_address(
            address in 0x02u8..=0xFE,
            id in any::<u32>(),
            other_address in any::<u8>(),
        ) {
            let mut state = DeviceRuntimeState::new();
            let ping = Frame::new(
                Address::Unicast(0x01),
                other_address,
                FramePayload::Ping,
            );
            prop_assert_eq!(try_assign_address_from_frame(&mut state, &ping), None);
            prop_assert_eq!(state.address(), None);

            let set_address = Frame::new(
                Address::Unicast(0x00),
                0x01,
                FramePayload::SetAddress { address, id },
            );
            prop_assert_eq!(
                try_assign_address_from_frame(&mut state, &set_address),
                Some(address)
            );
            prop_assert_eq!(state.address(), Some(address));
            prop_assert_eq!(state.take_next_control_seq().unwrap(), 0);
        }
    }

    #[test]
    fn control_sequence_wraps_to_zero() {
        let mut state = DeviceRuntimeState::new();
        state.assign_address(0x02);
        for expected in 0..=u16::MAX {
            assert_eq!(state.take_next_control_seq().unwrap(), expected);
        }
        assert_eq!(state.take_next_control_seq().unwrap(), 0);
    }

    #[test]
    fn device_hello_frame_preserves_the_descriptor() {
        let descriptor = DeviceDescriptor {
            device_id: 0x0123_4567_89AB_CDEF,
            device_kind: DeviceKind::ButtonPanel,
            firmware_version: Version {
                major: 1,
                minor: 2,
                patch: 3,
            },
            capabilities: Capabilities {
                displays: 1,
                controls: 8,
                features: FEATURE_CONTROL_EVENTS,
            },
        };
        assert_eq!(descriptor.protocol_version(), hcp::APP_PROTOCOL_VERSION);

        let frame = encode_set_frame(0x22, &build_device_hello_packet(descriptor)).unwrap();
        let FramePayload::Set(payload) = frame.payload() else {
            panic!("device hello is a Set frame");
        };
        assert_eq!(
            hcp::decode_set_packet(payload).unwrap(),
            hcp::AppPacketKind::DeviceHello(hcp::DeviceHello {
                device_id: descriptor.device_id,
                device_kind: descriptor.device_kind,
                protocol_version: descriptor.protocol_version(),
                firmware_version: descriptor.firmware_version,
                capabilities: descriptor.capabilities,
            })
        );
    }
}
