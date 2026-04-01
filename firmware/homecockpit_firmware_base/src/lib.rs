#![cfg_attr(not(test), no_std)]

use hcp::{
    APP_PROTOCOL_VERSION, AppPacketError, AppPacketKind, Capabilities, ControlEvent, ControlValue,
    DeviceHello, DeviceKind, Version, encode_set_packet,
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
}

impl DeviceRuntimeState {
    pub const fn new() -> Self {
        Self {
            address: None,
            next_control_seq: 0,
        }
    }

    pub fn address(&self) -> Option<u8> {
        self.address
    }

    pub fn assign_address(&mut self, address: u8) {
        self.address = Some(address);
        self.next_control_seq = 0;
    }

    pub fn take_next_control_seq(&mut self) -> Result<u16, FirmwareBaseError> {
        if self.address.is_none() {
            return Err(FirmwareBaseError::DeviceAddressUnassigned);
        }

        let seq = self.next_control_seq;
        self.next_control_seq = self.next_control_seq.wrapping_add(1);
        Ok(seq)
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

pub fn try_assign_address_from_frame(
    state: &mut DeviceRuntimeState,
    frame: &Frame,
) -> Option<u8> {
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
