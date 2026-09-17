use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Mutex},
};

use futures_executor::block_on;
use hcp::{
    AppPacketKind, Capabilities, DeviceKind, DisplayData, Version, decode_data_packet,
    decode_set_packet,
};
use homecockpit_firmware_base::{
    DeviceDescriptor, DeviceRuntimeState, FEATURE_CONTROL_EVENTS, build_button_control_event,
    build_device_hello_packet, encode_set_frame, try_assign_address_from_frame,
};
#[cfg(test)]
use imcp::frame::MAX_ENCODED_FRAME_SIZE;
use imcp::{
    Imcp,
    channel::{Receiver, Sender},
    error::ImcpError,
    frame::Frame,
};

#[cfg(unix)]
pub mod pty;

pub const CONTROL_COUNT: u16 = 40;
const CONTROL_COUNT_USIZE: usize = 40;
// A RequestDeviceHello frame produces an ACK and a DeviceHello response.
const FRAME_QUEUE_CAPACITY: usize = 7;
const RESERVED_PROTOCOL_FRAME_SLOTS: usize = 2;
pub const DEFAULT_DEVICE_ID: u64 = 0xDD10_0000_0000_0001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Press(u16),
    Release(u16),
    Tap(u16),
    Status,
    Help,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandParseError {
    Empty,
    UnknownCommand(String),
    MissingControlId,
    UnexpectedArgument,
    InvalidControlId(String),
}

impl fmt::Display for CommandParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "command is empty"),
            Self::UnknownCommand(command) => write!(formatter, "unknown command: {command}"),
            Self::MissingControlId => write!(formatter, "control-id is required"),
            Self::UnexpectedArgument => write!(formatter, "unexpected argument"),
            Self::InvalidControlId(value) => {
                write!(formatter, "invalid control-id '{value}'; expected 0..=39")
            }
        }
    }
}

pub fn parse_command(line: &str) -> Result<Command, CommandParseError> {
    let mut parts = line.split_whitespace();
    let Some(name) = parts.next() else {
        return Err(CommandParseError::Empty);
    };

    let command = match name.to_ascii_lowercase().as_str() {
        "press" => Command::Press(parse_control_id(parts.next())?),
        "release" => Command::Release(parse_control_id(parts.next())?),
        "tap" => Command::Tap(parse_control_id(parts.next())?),
        "status" => Command::Status,
        "help" => Command::Help,
        "quit" | "exit" => Command::Quit,
        _ => return Err(CommandParseError::UnknownCommand(name.to_string())),
    };

    if parts.next().is_some() {
        return Err(CommandParseError::UnexpectedArgument);
    }
    Ok(command)
}

fn parse_control_id(value: Option<&str>) -> Result<u16, CommandParseError> {
    let value = value.ok_or(CommandParseError::MissingControlId)?;
    let control_id = value
        .parse::<u16>()
        .map_err(|_| CommandParseError::InvalidControlId(value.to_string()))?;
    if control_id >= CONTROL_COUNT {
        return Err(CommandParseError::InvalidControlId(value.to_string()));
    }
    Ok(control_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameChannelError {
    Empty,
    Full,
    Poisoned,
}

#[derive(Clone)]
struct FrameSender {
    frames: Arc<Mutex<VecDeque<Frame>>>,
}

struct FrameReceiver {
    frames: Arc<Mutex<VecDeque<Frame>>>,
}

fn frame_channel() -> (FrameSender, FrameReceiver) {
    let frames = Arc::new(Mutex::new(VecDeque::new()));
    (
        FrameSender {
            frames: Arc::clone(&frames),
        },
        FrameReceiver { frames },
    )
}

impl FrameSender {
    fn clear(&self) -> Result<(), FrameChannelError> {
        self.frames
            .lock()
            .map_err(|_| FrameChannelError::Poisoned)?
            .clear();
        Ok(())
    }

    fn enqueue(&self, frame: Frame) -> Result<(), FrameChannelError> {
        let mut frames = self
            .frames
            .lock()
            .map_err(|_| FrameChannelError::Poisoned)?;
        if frames.len() >= FRAME_QUEUE_CAPACITY {
            return Err(FrameChannelError::Full);
        }
        frames.push_back(frame);
        Ok(())
    }

    fn enqueue_control(&self, frame: Frame) -> Result<(), FrameChannelError> {
        let mut frames = self
            .frames
            .lock()
            .map_err(|_| FrameChannelError::Poisoned)?;
        if frames.len() + RESERVED_PROTOCOL_FRAME_SLOTS >= FRAME_QUEUE_CAPACITY {
            return Err(FrameChannelError::Full);
        }
        frames.push_back(frame);
        Ok(())
    }

    fn has_control_capacity(&self, frame_count: usize) -> Result<bool, FrameChannelError> {
        let frames = self
            .frames
            .lock()
            .map_err(|_| FrameChannelError::Poisoned)?;
        Ok(frames
            .len()
            .saturating_add(frame_count)
            .saturating_add(RESERVED_PROTOCOL_FRAME_SLOTS)
            <= FRAME_QUEUE_CAPACITY)
    }
}

impl Sender for FrameSender {
    type Error = FrameChannelError;

    async fn send(&mut self, frame: Frame) -> Result<(), Self::Error> {
        self.enqueue(frame)
    }
}

impl Receiver for FrameReceiver {
    type Error = FrameChannelError;

    async fn receive(&mut self) -> Result<Frame, Self::Error> {
        self.frames
            .lock()
            .map_err(|_| FrameChannelError::Poisoned)?
            .pop_front()
            .ok_or(FrameChannelError::Empty)
    }
}

type Client<'a> = Imcp<'a, 'a, FrameReceiver, FrameSender>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceNotice {
    AddressAssigned(u8),
    DisplayData { data: DisplayData, accepted: bool },
    DeviceHelloRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockError {
    ChannelUnavailable,
    Protocol(String),
    NotReady,
    InvalidControlId(u16),
    AlreadyPressed(u16),
    AlreadyReleased(u16),
}

impl fmt::Display for MockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChannelUnavailable => write!(formatter, "mock frame channel is unavailable"),
            Self::Protocol(error) => write!(formatter, "IMCP protocol error: {error}"),
            Self::NotReady => write!(formatter, "device has no assigned IMCP address"),
            Self::InvalidControlId(id) => write!(formatter, "control-id {id} is outside 0..=39"),
            Self::AlreadyPressed(id) => write!(formatter, "control-id {id} is already pressed"),
            Self::AlreadyReleased(id) => write!(formatter, "control-id {id} is already released"),
        }
    }
}

pub struct MockDevice<'a> {
    client: Client<'a>,
    injector: FrameSender,
    runtime: DeviceRuntimeState,
    device_id: u64,
    join_id: u32,
    buttons: [bool; CONTROL_COUNT_USIZE],
}

impl<'a> MockDevice<'a> {
    pub fn new(device_id: u64, rx_buffer: &'a mut [u8], parser_buffer: &'a mut [u8]) -> Self {
        let (sender, receiver) = frame_channel();
        let injector = sender.clone();
        let client = Imcp::new_client(receiver, sender, rx_buffer, parser_buffer);
        Self {
            client,
            injector,
            runtime: DeviceRuntimeState::new(),
            device_id,
            join_id: join_id_from_device_id(device_id),
            buttons: [false; CONTROL_COUNT_USIZE],
        }
    }

    pub fn restart(&mut self) -> Result<(), MockError> {
        self.injector
            .clear()
            .map_err(|_| MockError::ChannelUnavailable)?;
        self.runtime = DeviceRuntimeState::new();
        self.buttons.fill(false);
        block_on(self.client.restart_client(self.join_id)).map_err(map_imcp_error)
    }

    pub fn address(&self) -> Option<u8> {
        self.runtime.address()
    }

    pub fn pressed_controls(&self) -> impl Iterator<Item = u16> + '_ {
        self.buttons
            .iter()
            .enumerate()
            .filter_map(|(index, pressed)| {
                if *pressed {
                    u16::try_from(index).ok()
                } else {
                    None
                }
            })
    }

    pub fn receive_bytes(&mut self, bytes: &[u8]) -> Result<Vec<DeviceNotice>, MockError> {
        let mut notices = Vec::new();
        let mut next_input = bytes;
        loop {
            let frame = block_on(self.client.read_tick(next_input)).map_err(map_imcp_error)?;
            next_input = &[];
            let Some(frame) = frame else {
                return Ok(notices);
            };
            if let Some(notice) = self.handle_frame(&frame)? {
                notices.push(notice);
            }
        }
    }

    pub fn next_wire_frame(&mut self) -> Result<Option<Vec<u8>>, MockError> {
        match block_on(self.client.write_tick()) {
            Ok(encoded) => Ok(Some(encoded.as_slice().to_vec())),
            Err(ImcpError::ReceiveError(FrameChannelError::Empty)) => Ok(None),
            Err(error) => Err(map_imcp_error(error)),
        }
    }

    pub fn press(&mut self, control_id: u16) -> Result<(), MockError> {
        self.set_button(control_id, true)
    }

    pub fn release(&mut self, control_id: u16) -> Result<(), MockError> {
        self.set_button(control_id, false)
    }

    pub fn tap(&mut self, control_id: u16) -> Result<(), MockError> {
        self.validate_control_id(control_id)?;
        if self.buttons[usize::from(control_id)] {
            return Err(MockError::AlreadyPressed(control_id));
        }
        if !self
            .injector
            .has_control_capacity(2)
            .map_err(|_| MockError::ChannelUnavailable)?
        {
            return Err(MockError::ChannelUnavailable);
        }
        self.enqueue_button_event(control_id, true)?;
        self.enqueue_button_event(control_id, false)?;
        self.buttons[usize::from(control_id)] = false;
        Ok(())
    }

    fn set_button(&mut self, control_id: u16, pressed: bool) -> Result<(), MockError> {
        self.validate_control_id(control_id)?;
        let index = usize::from(control_id);
        if self.buttons[index] == pressed {
            return if pressed {
                Err(MockError::AlreadyPressed(control_id))
            } else {
                Err(MockError::AlreadyReleased(control_id))
            };
        }
        self.enqueue_button_event(control_id, pressed)?;
        self.buttons[index] = pressed;
        Ok(())
    }

    fn validate_control_id(&self, control_id: u16) -> Result<(), MockError> {
        if control_id >= CONTROL_COUNT {
            return Err(MockError::InvalidControlId(control_id));
        }
        if self.address().is_none() {
            return Err(MockError::NotReady);
        }
        Ok(())
    }

    fn enqueue_button_event(&mut self, control_id: u16, pressed: bool) -> Result<(), MockError> {
        let address = self.address().ok_or(MockError::NotReady)?;
        if !self
            .injector
            .has_control_capacity(1)
            .map_err(|_| MockError::ChannelUnavailable)?
        {
            return Err(MockError::ChannelUnavailable);
        }
        let packet = build_button_control_event(&mut self.runtime, control_id, pressed)
            .map_err(|error| MockError::Protocol(format!("{error:?}")))?;
        let frame = encode_set_frame(address, &packet)
            .map_err(|error| MockError::Protocol(format!("{error:?}")))?;
        self.injector
            .enqueue_control(frame)
            .map_err(|_| MockError::ChannelUnavailable)
    }

    fn handle_frame(&mut self, frame: &Frame) -> Result<Option<DeviceNotice>, MockError> {
        if let Some(address) = try_assign_address_from_frame(&mut self.runtime, frame) {
            self.enqueue_device_hello()?;
            return Ok(Some(DeviceNotice::AddressAssigned(address)));
        }

        match frame.payload() {
            imcp::frame::FramePayload::Data(payload) => {
                let Ok(data) = decode_data_packet(payload.as_slice()) else {
                    return Ok(None);
                };
                let accepted = self.runtime.accept_display_data(&data);
                Ok(Some(DeviceNotice::DisplayData { data, accepted }))
            }
            imcp::frame::FramePayload::Set(payload) => {
                let Ok(AppPacketKind::ControlEvent(event)) = decode_set_packet(payload.as_slice())
                else {
                    return Ok(None);
                };
                if matches!(event.event, hcp::ControlValue::RequestDeviceHello) {
                    self.enqueue_device_hello()?;
                    Ok(Some(DeviceNotice::DeviceHelloRequested))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    fn enqueue_device_hello(&self) -> Result<(), MockError> {
        let address = self.address().ok_or(MockError::NotReady)?;
        let hello = build_device_hello_packet(DeviceDescriptor {
            device_id: self.device_id,
            device_kind: DeviceKind::UpperPanelDdi,
            firmware_version: Version {
                major: 0,
                minor: 1,
                patch: 0,
            },
            capabilities: Capabilities {
                displays: 0,
                controls: CONTROL_COUNT,
                features: FEATURE_CONTROL_EVENTS,
            },
        });
        let frame = encode_set_frame(address, &hello)
            .map_err(|error| MockError::Protocol(format!("{error:?}")))?;
        self.injector
            .enqueue(frame)
            .map_err(|_| MockError::ChannelUnavailable)
    }
}

fn join_id_from_device_id(device_id: u64) -> u32 {
    let bytes = device_id.to_le_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        ^ u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])
}

fn map_imcp_error(error: ImcpError<FrameChannelError, FrameChannelError>) -> MockError {
    MockError::Protocol(format!("{error:?}"))
}

#[cfg(test)]
pub(crate) fn encode_frame(frame: &Frame) -> Result<Vec<u8>, MockError> {
    let mut bytes = [0u8; MAX_ENCODED_FRAME_SIZE];
    let length = frame
        .encode(&mut bytes)
        .map_err(|error| MockError::Protocol(format!("{error:?}")))?;
    Ok(bytes[..length].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hcp::{
        AppPacketKind, ByteEncoding, ControlEvent, ControlValue, DisplayPayload, DisplayTarget,
        decode_set_packet, encode_data_packet, encode_set_packet,
    };
    use imcp::{
        frame::{Address, FramePayload},
        parser::FrameParser,
    };

    fn device() -> MockDevice<'static> {
        let rx_buffer = Box::leak(Box::new([0u8; 256]));
        let parser_buffer = Box::leak(Box::new([0u8; 256]));
        MockDevice::new(DEFAULT_DEVICE_ID, rx_buffer, parser_buffer)
    }

    fn decode_frame(bytes: &[u8]) -> Frame {
        let rx_buffer = Box::leak(Box::new([0u8; 256]));
        let parser_buffer = Box::leak(Box::new([0u8; 256]));
        let mut parser = FrameParser::new(rx_buffer, parser_buffer);
        assert!(parser.write_data(bytes).is_ok());
        match parser.next_frame() {
            Some(Ok(frame)) => frame,
            other => panic!("expected frame, got {other:?}"),
        }
    }

    fn assign_address(device: &mut MockDevice<'_>, address: u8) {
        assert!(device.restart().is_ok());
        let join = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        let FramePayload::Join(join_id) = join.payload() else {
            panic!("expected Join");
        };
        let set_address = Frame::new(
            Address::Unicast(0x00),
            0x01,
            FramePayload::SetAddress {
                address,
                id: *join_id,
            },
        );
        let notices = device
            .receive_bytes(&encode_frame(&set_address).unwrap())
            .unwrap();
        assert_eq!(notices, vec![DeviceNotice::AddressAssigned(address)]);
    }

    #[test]
    fn parses_interactive_commands_and_rejects_bad_ids() {
        assert_eq!(parse_command("press 0"), Ok(Command::Press(0)));
        assert_eq!(parse_command("tap 39"), Ok(Command::Tap(39)));
        assert!(matches!(
            parse_command("release 40"),
            Err(CommandParseError::InvalidControlId(_))
        ));
        assert_eq!(parse_command("exit"), Ok(Command::Quit));
    }

    #[test]
    fn join_assignment_ack_and_device_hello_follow_protocol() {
        let mut device = device();
        assign_address(&mut device, 0x02);

        let assignment_ack = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        assert_eq!(
            assignment_ack,
            Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ack(0x00))
        );

        let hello_frame = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        let FramePayload::Set(payload) = hello_frame.payload() else {
            panic!("expected DeviceHello Set");
        };
        let hello = decode_set_packet(payload.as_slice()).unwrap();
        let AppPacketKind::DeviceHello(hello) = hello else {
            panic!("expected DeviceHello");
        };
        assert_eq!(hello.device_kind, DeviceKind::UpperPanelDdi);
        assert_eq!(hello.device_id, DEFAULT_DEVICE_ID);
        assert_eq!(hello.capabilities.controls, CONTROL_COUNT);
        assert_eq!(hello.capabilities.displays, 0);
    }

    #[test]
    fn button_events_are_serialized_and_sequenced() {
        let mut device = device();
        assign_address(&mut device, 0x02);
        let _assignment_ack = device.next_wire_frame().unwrap().unwrap();
        let _hello = device.next_wire_frame().unwrap().unwrap();
        let hello_ack = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ack(0x01));
        assert!(
            device
                .receive_bytes(&encode_frame(&hello_ack).unwrap())
                .is_ok()
        );

        assert!(device.tap(7).is_ok());
        let pressed = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        let FramePayload::Set(payload) = pressed.payload() else {
            panic!("expected pressed Set");
        };
        let AppPacketKind::ControlEvent(event) = decode_set_packet(payload.as_slice()).unwrap()
        else {
            panic!("expected ControlEvent");
        };
        assert_eq!(event.seq, 0);
        assert_eq!(event.control_id, 7);
        assert_eq!(event.event, ControlValue::Button { pressed: true });

        let event_ack = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ack(0x01));
        assert!(
            device
                .receive_bytes(&encode_frame(&event_ack).unwrap())
                .is_ok()
        );
        let released = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        let FramePayload::Set(payload) = released.payload() else {
            panic!("expected released Set");
        };
        let AppPacketKind::ControlEvent(event) = decode_set_packet(payload.as_slice()).unwrap()
        else {
            panic!("expected ControlEvent");
        };
        assert_eq!(event.seq, 1);
        assert_eq!(event.event, ControlValue::Button { pressed: false });
    }

    #[test]
    fn ping_is_answered_with_pong() {
        let mut device = device();
        assign_address(&mut device, 0x02);
        let _assignment_ack = device.next_wire_frame().unwrap().unwrap();
        let _hello = device.next_wire_frame().unwrap().unwrap();
        let hello_ack = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ack(0x01));
        assert!(
            device
                .receive_bytes(&encode_frame(&hello_ack).unwrap())
                .is_ok()
        );

        let ping = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ping);
        assert!(device.receive_bytes(&encode_frame(&ping).unwrap()).is_ok());
        let pong = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        assert_eq!(
            pong,
            Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Pong)
        );
    }

    #[test]
    fn display_data_reports_fresh_and_stale_sequences() {
        let mut device = device();
        assign_address(&mut device, 0x02);
        let _assignment_ack = device.next_wire_frame().unwrap().unwrap();
        let _hello = device.next_wire_frame().unwrap().unwrap();

        let display = DisplayData {
            seq: 12,
            target: DisplayTarget::Indicator(3),
            payload: DisplayPayload::Bytes {
                encoding: ByteEncoding::SegmentMap,
                data: [1, 2, 3].into_iter().collect(),
            },
        };
        let payload = encode_data_packet(&display).unwrap();
        let frame = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Data(payload));
        let wire = encode_frame(&frame).unwrap();

        assert_eq!(
            device.receive_bytes(&wire).unwrap(),
            vec![DeviceNotice::DisplayData {
                data: display.clone(),
                accepted: true,
            }]
        );
        assert_eq!(
            device.receive_bytes(&wire).unwrap(),
            vec![DeviceNotice::DisplayData {
                data: display,
                accepted: false,
            }]
        );
    }

    #[test]
    fn request_device_hello_is_acked_and_replied_to() {
        let mut device = device();
        assign_address(&mut device, 0x02);
        let _assignment_ack = device.next_wire_frame().unwrap().unwrap();
        let _hello = device.next_wire_frame().unwrap().unwrap();
        let hello_ack = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ack(0x01));
        assert!(
            device
                .receive_bytes(&encode_frame(&hello_ack).unwrap())
                .is_ok()
        );

        let request = encode_set_packet(&AppPacketKind::ControlEvent(ControlEvent {
            seq: 4,
            control_id: hcp::CONTROL_ID_REQUEST_DEVICE_HELLO,
            event: ControlValue::RequestDeviceHello,
        }))
        .unwrap();
        let request = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Set(request));
        assert_eq!(
            device
                .receive_bytes(&encode_frame(&request).unwrap())
                .unwrap(),
            vec![DeviceNotice::DeviceHelloRequested]
        );

        let ack = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        assert_eq!(
            ack,
            Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ack(0x02))
        );
        let hello = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        assert!(matches!(hello.payload(), FramePayload::Set(_)));
    }

    #[test]
    fn protocol_responses_fit_when_normal_event_queue_is_full() {
        let mut device = device();
        assign_address(&mut device, 0x02);
        let _assignment_ack = device.next_wire_frame().unwrap().unwrap();
        let _hello = device.next_wire_frame().unwrap().unwrap();
        let hello_ack = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ack(0x01));
        device
            .receive_bytes(&encode_frame(&hello_ack).unwrap())
            .unwrap();

        for control_id in 0..5 {
            assert!(device.press(control_id).is_ok());
        }
        assert_eq!(
            device.press(5),
            Err(MockError::ChannelUnavailable),
            "normal events must leave two protocol slots available"
        );

        let request = encode_set_packet(&AppPacketKind::ControlEvent(ControlEvent {
            seq: 4,
            control_id: hcp::CONTROL_ID_REQUEST_DEVICE_HELLO,
            event: ControlValue::RequestDeviceHello,
        }))
        .unwrap();
        let request = Frame::new(Address::Broadcast, 0x01, FramePayload::Set(request));
        assert_eq!(
            device
                .receive_bytes(&encode_frame(&request).unwrap())
                .unwrap(),
            vec![DeviceNotice::DeviceHelloRequested]
        );

        for expected_seq in 0u16..5 {
            let event = decode_frame(&device.next_wire_frame().unwrap().unwrap());
            let FramePayload::Set(payload) = event.payload() else {
                panic!("expected button event Set");
            };
            assert!(matches!(
                decode_set_packet(payload.as_slice()),
                Ok(AppPacketKind::ControlEvent(event)) if event.seq == expected_seq
            ));
            let event_ack = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ack(0x01));
            device
                .receive_bytes(&encode_frame(&event_ack).unwrap())
                .unwrap();
        }

        let request_ack = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        assert_eq!(
            request_ack,
            Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ack(0xFF))
        );
        let hello = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        let FramePayload::Set(payload) = hello.payload() else {
            panic!("expected DeviceHello Set");
        };
        assert!(matches!(
            decode_set_packet(payload.as_slice()),
            Ok(AppPacketKind::DeviceHello(hello)) if hello.device_id == DEFAULT_DEVICE_ID
        ));

        let hello_ack = Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ack(0x01));
        device
            .receive_bytes(&encode_frame(&hello_ack).unwrap())
            .unwrap();
        assert!(device.press(5).is_ok());
        let event = decode_frame(&device.next_wire_frame().unwrap().unwrap());
        let FramePayload::Set(payload) = event.payload() else {
            panic!("expected button event Set");
        };
        assert!(matches!(
            decode_set_packet(payload.as_slice()),
            Ok(AppPacketKind::ControlEvent(event)) if event.seq == 5
        ));
    }
}
