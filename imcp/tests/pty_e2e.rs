#![cfg(unix)]

use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Read, Write},
    os::fd::{AsRawFd, FromRawFd, RawFd},
    ptr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures::executor::block_on;
use imcp::{
    EOF, ESC, ESC_XOR, Imcp, SOF,
    channel::{Receiver, Sender},
    error::{DecodeError, ImcpError},
    frame::{Address, Frame, FramePayload},
    frame::{MAX_ENCODED_FRAME_SIZE, MAX_PAYLOAD_SIZE},
};

const READ_TIMEOUT: Duration = Duration::from_secs(2);
const TEST_BUFFER_SIZE: usize = 256;

type TestImcp = Imcp<'static, 'static, QueueReceiver, QueueSender>;
type TestImcpError = ImcpError<QueueError, QueueError>;

#[derive(Clone)]
struct QueueSender {
    frames: Arc<Mutex<VecDeque<Frame>>>,
}

struct QueueReceiver {
    frames: Arc<Mutex<VecDeque<Frame>>>,
}

#[derive(Debug)]
enum QueueError {
    Empty,
    Poisoned,
}

fn frame_channel() -> (QueueSender, QueueReceiver) {
    let frames = Arc::new(Mutex::new(VecDeque::new()));
    (
        QueueSender {
            frames: Arc::clone(&frames),
        },
        QueueReceiver { frames },
    )
}

impl Sender for QueueSender {
    type Error = QueueError;

    async fn send(&mut self, frame: Frame) -> Result<(), Self::Error> {
        self.frames
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .push_back(frame);
        Ok(())
    }
}

impl Receiver for QueueReceiver {
    type Error = QueueError;

    async fn receive(&mut self) -> Result<Frame, Self::Error> {
        self.frames
            .lock()
            .map_err(|_| QueueError::Poisoned)?
            .pop_front()
            .ok_or(QueueError::Empty)
    }
}

fn new_master() -> (TestImcp, QueueSender) {
    let (sender, receiver) = frame_channel();
    let injector = sender.clone();
    let rx_buffer = Box::leak(Box::new([0u8; TEST_BUFFER_SIZE]));
    let frame_buffer = Box::leak(Box::new([0u8; TEST_BUFFER_SIZE]));
    let master = Imcp::new_master(receiver, sender, rx_buffer, frame_buffer);
    (master, injector)
}

fn new_client() -> (TestImcp, QueueSender) {
    let (sender, receiver) = frame_channel();
    let injector = sender.clone();
    let rx_buffer = Box::leak(Box::new([0u8; TEST_BUFFER_SIZE]));
    let frame_buffer = Box::leak(Box::new([0u8; TEST_BUFFER_SIZE]));
    let client = Imcp::new_client(receiver, sender, rx_buffer, frame_buffer);
    (client, injector)
}

struct PtyPair {
    master: File,
    slave: File,
}

impl PtyPair {
    fn new() -> io::Result<Self> {
        let mut master_fd: RawFd = -1;
        let mut slave_fd: RawFd = -1;
        let result = unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };

        if result == -1 {
            return Err(io::Error::last_os_error());
        }

        let master = unsafe { File::from_raw_fd(master_fd) };
        let slave = unsafe { File::from_raw_fd(slave_fd) };
        set_raw_mode(slave.as_raw_fd())?;

        Ok(Self { master, slave })
    }
}

fn set_raw_mode(fd: RawFd) -> io::Result<()> {
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    let result = unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }

    let mut termios = unsafe { termios.assume_init() };
    unsafe { libc::cfmakeraw(&mut termios) };

    let result = unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok(())
}

fn read_byte_with_deadline(file: &mut File, deadline: Instant) -> io::Result<u8> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for an IMCP byte",
            ));
        }

        let max_timeout_ms = u128::try_from(i32::MAX)
            .map_err(|_| io::Error::other("failed to calculate poll timeout"))?;
        let timeout_ms = i32::try_from(remaining.as_millis().clamp(1, max_timeout_ms))
            .map_err(|_| io::Error::other("poll timeout is out of range"))?;
        let mut poll_fd = libc::pollfd {
            fd: file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };

        let result = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if result == 0 {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for an IMCP byte",
            ));
        }

        if poll_fd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) == 0 {
            continue;
        }

        let mut byte = [0u8; 1];
        match file.read(&mut byte) {
            Ok(1) => return Ok(byte[0]),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "PTY closed before an IMCP frame was received",
                ));
            }
            Ok(_) => {
                return Err(io::Error::other(
                    "one-byte read returned more than one byte",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

fn read_next_frame(file: &mut File, node: &mut TestImcp) -> Result<Frame, String> {
    let deadline = Instant::now() + READ_TIMEOUT;

    loop {
        let byte = [read_byte_with_deadline(file, deadline)
            .map_err(|error| format!("read IMCP byte: {error}"))?];
        let frame = block_on(node.read_tick(&byte))
            .map_err(|error| format!("decode IMCP byte: {error:?}"))?;
        if let Some(frame) = frame {
            return Ok(frame);
        }
    }
}

fn read_wire_bytes(file: &mut File, length: usize) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + READ_TIMEOUT;
    let mut bytes = Vec::with_capacity(length);

    for _ in 0..length {
        bytes.push(
            read_byte_with_deadline(file, deadline)
                .map_err(|error| format!("read IMCP byte: {error}"))?,
        );
    }

    Ok(bytes)
}

fn deliver_wire_bytes(node: &mut TestImcp, bytes: &[u8]) -> Result<Option<Frame>, TestImcpError> {
    let mut last_frame = None;

    for byte in bytes {
        last_frame = block_on(node.read_tick(std::slice::from_ref(byte)))?;
    }

    Ok(last_frame)
}

fn write_next_frame(file: &mut File, node: &mut TestImcp) -> Result<Vec<u8>, String> {
    let encoded =
        block_on(node.write_tick()).map_err(|error| format!("encode IMCP frame: {error:?}"))?;
    let bytes = encoded.as_slice().to_vec();
    file.write_all(&bytes)
        .map_err(|error| format!("write IMCP frame: {error}"))?;
    file.flush()
        .map_err(|error| format!("flush IMCP frame: {error}"))?;
    Ok(bytes)
}

fn encode_frame(frame: &Frame) -> Result<Vec<u8>, String> {
    let mut encoded = [0u8; MAX_ENCODED_FRAME_SIZE];
    let encoded_len = frame
        .encode(&mut encoded)
        .map_err(|error| format!("encode IMCP frame: {error:?}"))?;
    Ok(encoded[..encoded_len].to_vec())
}

fn write_wire_bytes(file: &mut File, bytes: &[u8]) -> Result<(), String> {
    file.write_all(bytes)
        .map_err(|error| format!("write IMCP bytes: {error}"))?;
    file.flush()
        .map_err(|error| format!("flush IMCP bytes: {error}"))?;
    Ok(())
}

fn contains_pair(bytes: &[u8], first: u8, second: u8) -> bool {
    bytes
        .windows(2)
        .any(|window| window[0] == first && window[1] == second)
}

fn payload_from(bytes: &[u8]) -> heapless::Vec<u8, MAX_PAYLOAD_SIZE> {
    let mut payload = heapless::Vec::new();
    for byte in bytes {
        if payload.push(*byte).is_err() {
            break;
        }
    }
    payload
}

fn join_over_wire(
    master_io: &mut File,
    device_io: &mut File,
    master: &mut TestImcp,
    client: &mut TestImcp,
    join_id: u32,
) -> Result<u8, String> {
    block_on(client.send_join(join_id)).map_err(|error| format!("queue client Join: {error:?}"))?;
    write_next_frame(device_io, client)?;

    let join = read_next_frame(master_io, master)?;
    assert_eq!(
        join,
        Frame::new(Address::Unicast(0x01), 0x00, FramePayload::Join(join_id))
    );

    write_next_frame(master_io, master)?;
    let set_address = read_next_frame(device_io, client)?;
    let assigned_address = match set_address.payload() {
        FramePayload::SetAddress { address, id } => {
            assert_eq!(*id, join_id);
            *address
        }
        payload => return Err(format!("expected SetAddress, got {payload:?}")),
    };

    write_next_frame(device_io, client)?;
    let assignment_ack = read_next_frame(master_io, master)?;
    assert_eq!(
        assignment_ack,
        Frame::new(
            Address::Unicast(0x01),
            assigned_address,
            FramePayload::Ack(0x00),
        )
    );

    Ok(assigned_address)
}

#[test]
fn lost_assignment_frame_is_retried_over_a_posix_pty_wire() -> Result<(), String> {
    let pty = PtyPair::new().map_err(|error| format!("create POSIX PTY: {error}"))?;
    let mut device_io = pty.master;
    let mut master_io = pty.slave;
    let (mut master, _) = new_master();
    let (mut client, _) = new_client();

    block_on(client.send_join(0xCAFE_BABE))
        .map_err(|error| format!("queue client Join: {error:?}"))?;
    write_next_frame(&mut device_io, &mut client)?;
    let join = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        join,
        Frame::new(
            Address::Unicast(0x01),
            0x00,
            FramePayload::Join(0xCAFE_BABE)
        )
    );

    let first_set_address = write_next_frame(&mut master_io, &mut master)?;
    let _dropped_set_address = read_wire_bytes(&mut device_io, first_set_address.len())?;

    // The client retransmits its pending Join after the first assignment frame is lost.
    write_next_frame(&mut device_io, &mut client)?;
    let retried_join = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(retried_join, join);

    let retried_set_address = write_next_frame(&mut master_io, &mut master)?;
    assert_eq!(retried_set_address, first_set_address);
    let set_address = read_next_frame(&mut device_io, &mut client)?;
    assert_eq!(
        set_address,
        Frame::new(
            Address::Unicast(0x00),
            0x01,
            FramePayload::SetAddress {
                address: 0x02,
                id: 0xCAFE_BABE,
            },
        )
    );

    write_next_frame(&mut device_io, &mut client)?;
    let assignment_ack = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        assignment_ack,
        Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ack(0x00))
    );

    Ok(())
}

#[test]
fn lost_assignment_ack_is_recovered_over_a_posix_pty_wire() -> Result<(), String> {
    let pty = PtyPair::new().map_err(|error| format!("create POSIX PTY: {error}"))?;
    let mut device_io = pty.master;
    let mut master_io = pty.slave;
    let (mut master, _) = new_master();
    let (mut client, _) = new_client();

    block_on(client.send_join(0x1234_ABCD))
        .map_err(|error| format!("queue client Join: {error:?}"))?;
    write_next_frame(&mut device_io, &mut client)?;
    let join = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        join,
        Frame::new(
            Address::Unicast(0x01),
            0x00,
            FramePayload::Join(0x1234_ABCD)
        )
    );

    let first_set_address = write_next_frame(&mut master_io, &mut master)?;
    let first_set_bytes = read_wire_bytes(&mut device_io, first_set_address.len())?;
    let first_set = deliver_wire_bytes(&mut client, &first_set_bytes)
        .map_err(|error| format!("deliver first SetAddress: {error:?}"))?;
    assert!(first_set.is_some());

    // The first ACK is lost on the wire, so the master keeps its SetAddress pending.
    let first_ack = write_next_frame(&mut device_io, &mut client)
        .map_err(|error| format!("write first assignment ACK: {error}"))?;
    let _dropped_ack = read_wire_bytes(&mut master_io, first_ack.len())?;

    let retried_set_address = write_next_frame(&mut master_io, &mut master)
        .map_err(|error| format!("write retried SetAddress: {error}"))?;
    assert_eq!(retried_set_address, first_set_address);
    let retried_set_bytes = read_wire_bytes(&mut device_io, retried_set_address.len())?;
    let duplicate_set = deliver_wire_bytes(&mut client, &retried_set_bytes)
        .map_err(|error| format!("deliver duplicate SetAddress: {error:?}"))?;
    assert!(duplicate_set.is_none());

    write_next_frame(&mut device_io, &mut client)
        .map_err(|error| format!("write retry assignment ACK: {error}"))?;
    let retry_ack = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        retry_ack,
        Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ack(0x00))
    );

    Ok(())
}

#[test]
fn multiple_clients_get_distinct_addresses_and_filter_unicast_frames() -> Result<(), String> {
    let pty = PtyPair::new().map_err(|error| format!("create POSIX PTY: {error}"))?;
    let mut device_io = pty.master;
    let mut master_io = pty.slave;
    let (mut master, mut master_injector) = new_master();
    let (mut first_client, _) = new_client();
    let (mut second_client, _) = new_client();

    let first_address = join_over_wire(
        &mut master_io,
        &mut device_io,
        &mut master,
        &mut first_client,
        0x1111_0001,
    )?;
    let second_address = join_over_wire(
        &mut master_io,
        &mut device_io,
        &mut master,
        &mut second_client,
        0x2222_0002,
    )?;
    assert_eq!(first_address, 0x02);
    assert_eq!(second_address, 0x03);

    block_on(master_injector.send(Frame::new(
        Address::Unicast(first_address),
        0x01,
        FramePayload::Ping,
    )))
    .map_err(|error| format!("queue first-client Ping: {error:?}"))?;
    let ping_bytes = write_next_frame(&mut master_io, &mut master)?;
    let ping_wire = read_wire_bytes(&mut device_io, ping_bytes.len())?;
    let first_seen = deliver_wire_bytes(&mut first_client, &ping_wire)
        .map_err(|error| format!("deliver Ping to first client: {error:?}"))?;
    let second_seen = deliver_wire_bytes(&mut second_client, &ping_wire)
        .map_err(|error| format!("deliver Ping to second client: {error:?}"))?;
    assert!(first_seen.is_some());
    assert!(second_seen.is_none());

    write_next_frame(&mut device_io, &mut first_client)?;
    let first_pong = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        first_pong,
        Frame::new(Address::Unicast(0x01), first_address, FramePayload::Pong,)
    );

    block_on(master_injector.send(Frame::new(
        Address::Unicast(second_address),
        0x01,
        FramePayload::Ping,
    )))
    .map_err(|error| format!("queue second-client Ping: {error:?}"))?;
    let ping_bytes = write_next_frame(&mut master_io, &mut master)?;
    let ping_wire = read_wire_bytes(&mut device_io, ping_bytes.len())?;
    let first_seen = deliver_wire_bytes(&mut first_client, &ping_wire)
        .map_err(|error| format!("deliver second Ping to first client: {error:?}"))?;
    let second_seen = deliver_wire_bytes(&mut second_client, &ping_wire)
        .map_err(|error| format!("deliver second Ping to second client: {error:?}"))?;
    assert!(first_seen.is_none());
    assert!(second_seen.is_some());

    write_next_frame(&mut device_io, &mut second_client)?;
    let second_pong = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        second_pong,
        Frame::new(Address::Unicast(0x01), second_address, FramePayload::Pong,)
    );

    Ok(())
}

#[test]
fn broadcast_frames_are_delivered_and_acknowledged_over_a_posix_pty_wire() -> Result<(), String> {
    let pty = PtyPair::new().map_err(|error| format!("create POSIX PTY: {error}"))?;
    let mut device_io = pty.master;
    let mut master_io = pty.slave;
    let (mut master, mut master_injector) = new_master();
    let (mut client, _) = new_client();
    join_over_wire(
        &mut master_io,
        &mut device_io,
        &mut master,
        &mut client,
        0xBADC_0FFE,
    )?;

    block_on(master_injector.send(Frame::new(Address::Broadcast, 0x01, FramePayload::Ping)))
        .map_err(|error| format!("queue broadcast Ping: {error:?}"))?;
    write_next_frame(&mut master_io, &mut master)?;
    let broadcast_ping = read_next_frame(&mut device_io, &mut client)?;
    assert_eq!(
        broadcast_ping,
        Frame::new(Address::Broadcast, 0x01, FramePayload::Ping)
    );
    write_next_frame(&mut device_io, &mut client)?;
    let pong = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        pong,
        Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Pong)
    );

    block_on(master_injector.send(Frame::new(
        Address::Broadcast,
        0x01,
        FramePayload::Set(payload_from(&[0xAA, SOF, 0x55])),
    )))
    .map_err(|error| format!("queue broadcast Set: {error:?}"))?;
    write_next_frame(&mut master_io, &mut master)?;
    let broadcast_set = read_next_frame(&mut device_io, &mut client)?;
    assert!(matches!(broadcast_set.payload(), FramePayload::Set(_)));
    write_next_frame(&mut device_io, &mut client)?;
    let broadcast_ack = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        broadcast_ack,
        Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ack(0xFF))
    );

    Ok(())
}

#[test]
fn malformed_wire_frames_are_rejected_and_following_frames_recover() -> Result<(), String> {
    let pty = PtyPair::new().map_err(|error| format!("create POSIX PTY: {error}"))?;
    let mut device_io = pty.master;
    let mut master_io = pty.slave;
    let (mut master, mut master_injector) = new_master();
    let (mut client, _) = new_client();
    join_over_wire(
        &mut master_io,
        &mut device_io,
        &mut master,
        &mut client,
        0xDEAD_BEEF,
    )?;

    let valid_data = encode_frame(&Frame::new(
        Address::Unicast(0x02),
        0x01,
        FramePayload::Data(payload_from(&[SOF])),
    ))?;
    let mut invalid_checksum = valid_data.clone();
    let checksum_index = invalid_checksum
        .len()
        .checked_sub(2)
        .ok_or_else(|| "encoded frame is too short to corrupt".to_string())?;
    invalid_checksum[checksum_index] ^= 0x01;
    write_wire_bytes(&mut master_io, &invalid_checksum)?;
    let invalid_checksum_wire = read_wire_bytes(&mut device_io, invalid_checksum.len())?;
    let checksum_result = deliver_wire_bytes(&mut client, &invalid_checksum_wire);
    assert!(matches!(
        checksum_result,
        Err(ImcpError::DecodeError(DecodeError::InvalidChecksum))
    ));

    let mut invalid_escape = valid_data;
    let escaped_sof = invalid_escape
        .windows(2)
        .position(|window| window[0] == ESC && window[1] == (SOF ^ ESC_XOR))
        .ok_or_else(|| "encoded frame did not contain an escaped SOF".to_string())?;
    invalid_escape[escaped_sof + 1] = EOF;
    let invalid_escape_len = escaped_sof + 2;
    write_wire_bytes(&mut master_io, &invalid_escape[..invalid_escape_len])?;
    let invalid_escape_wire = read_wire_bytes(&mut device_io, invalid_escape_len)?;
    let escape_result = deliver_wire_bytes(&mut client, &invalid_escape_wire);
    assert!(matches!(
        escape_result,
        Err(ImcpError::DecodeError(DecodeError::InvalidEscapeSequence))
    ));

    block_on(master_injector.send(Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ping)))
        .map_err(|error| format!("queue recovery Ping: {error:?}"))?;
    write_next_frame(&mut master_io, &mut master)?;
    let ping = read_next_frame(&mut device_io, &mut client)?;
    assert_eq!(
        ping,
        Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ping)
    );
    write_next_frame(&mut device_io, &mut client)?;
    let pong = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        pong,
        Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Pong)
    );

    Ok(())
}

#[test]
fn maximum_data_payload_with_stuffing_survives_a_posix_pty_wire() -> Result<(), String> {
    let pty = PtyPair::new().map_err(|error| format!("create POSIX PTY: {error}"))?;
    let mut device_io = pty.master;
    let mut master_io = pty.slave;
    let (mut master, mut master_injector) = new_master();
    let (mut client, _) = new_client();
    join_over_wire(
        &mut master_io,
        &mut device_io,
        &mut master,
        &mut client,
        0xFACE_CAFE,
    )?;

    let mut max_payload_bytes = [0u8; MAX_PAYLOAD_SIZE];
    for (index, byte) in max_payload_bytes.iter_mut().enumerate() {
        *byte = match index % 3 {
            0 => SOF,
            1 => EOF,
            _ => ESC,
        };
    }
    let max_payload = payload_from(&max_payload_bytes);
    block_on(master_injector.send(Frame::new(
        Address::Unicast(0x02),
        0x01,
        FramePayload::Data(max_payload.clone()),
    )))
    .map_err(|error| format!("queue maximum Data: {error:?}"))?;
    let encoded = write_next_frame(&mut master_io, &mut master)?;
    assert!(encoded.len() <= MAX_ENCODED_FRAME_SIZE);

    let data = read_next_frame(&mut device_io, &mut client)?;
    assert_eq!(
        data,
        Frame::new(
            Address::Unicast(0x02),
            0x01,
            FramePayload::Data(max_payload),
        )
    );

    Ok(())
}

#[test]
fn master_and_client_exchange_frames_over_a_posix_pty_wire() -> Result<(), String> {
    let pty = PtyPair::new().map_err(|error| format!("create POSIX PTY: {error}"))?;
    let mut device_io = pty.master;
    let mut master_io = pty.slave;
    let (mut master, mut master_injector) = new_master();
    let (mut client, mut client_injector) = new_client();

    block_on(client.send_join(0xCAFE_BABE))
        .map_err(|error| format!("queue client Join: {error:?}"))?;
    write_next_frame(&mut device_io, &mut client)?;

    let join = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        join,
        Frame::new(
            Address::Unicast(0x01),
            0x00,
            FramePayload::Join(0xCAFE_BABE)
        )
    );

    write_next_frame(&mut master_io, &mut master)?;
    let set_address = read_next_frame(&mut device_io, &mut client)?;
    assert_eq!(
        set_address,
        Frame::new(
            Address::Unicast(0x00),
            0x01,
            FramePayload::SetAddress {
                address: 0x02,
                id: 0xCAFE_BABE,
            },
        )
    );

    write_next_frame(&mut device_io, &mut client)?;
    let assignment_ack = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        assignment_ack,
        Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ack(0x00),)
    );

    block_on(master_injector.send(Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ping)))
        .map_err(|error| format!("queue master Ping: {error:?}"))?;
    write_next_frame(&mut master_io, &mut master)?;

    let ping = read_next_frame(&mut device_io, &mut client)?;
    assert_eq!(
        ping,
        Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ping)
    );

    write_next_frame(&mut device_io, &mut client)?;
    let pong = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        pong,
        Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Pong)
    );

    let set_payload = payload_from(&[SOF, EOF, ESC, 0x42]);
    block_on(master_injector.send(Frame::new(
        Address::Unicast(0x02),
        0x01,
        FramePayload::Set(set_payload.clone()),
    )))
    .map_err(|error| format!("queue master Set: {error:?}"))?;
    let encoded_set = write_next_frame(&mut master_io, &mut master)?;
    assert!(contains_pair(&encoded_set, ESC, SOF ^ ESC_XOR));
    assert!(contains_pair(&encoded_set, ESC, EOF ^ ESC_XOR));
    assert!(contains_pair(&encoded_set, ESC, ESC ^ ESC_XOR));

    let set = read_next_frame(&mut device_io, &mut client)?;
    assert_eq!(
        set,
        Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Set(set_payload),)
    );

    write_next_frame(&mut device_io, &mut client)?;
    let set_ack = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        set_ack,
        Frame::new(Address::Unicast(0x01), 0x02, FramePayload::Ack(0x02))
    );

    let client_payload = payload_from(&[0x10, SOF, 0x20]);
    block_on(client_injector.send(Frame::new(
        Address::Unicast(0x01),
        0x02,
        FramePayload::Data(client_payload.clone()),
    )))
    .map_err(|error| format!("queue client Data: {error:?}"))?;
    write_next_frame(&mut device_io, &mut client)?;

    let data = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        data,
        Frame::new(
            Address::Unicast(0x01),
            0x02,
            FramePayload::Data(client_payload),
        )
    );

    let client_set_payload = payload_from(&[0x01, ESC, SOF, EOF, 0x02]);
    block_on(client_injector.send(Frame::new(
        Address::Unicast(0x01),
        0x02,
        FramePayload::Set(client_set_payload.clone()),
    )))
    .map_err(|error| format!("queue client Set: {error:?}"))?;
    let encoded_client_set = write_next_frame(&mut device_io, &mut client)?;
    assert!(contains_pair(&encoded_client_set, ESC, SOF ^ ESC_XOR));
    assert!(contains_pair(&encoded_client_set, ESC, EOF ^ ESC_XOR));
    assert!(contains_pair(&encoded_client_set, ESC, ESC ^ ESC_XOR));

    let client_set = read_next_frame(&mut master_io, &mut master)?;
    assert_eq!(
        client_set,
        Frame::new(
            Address::Unicast(0x01),
            0x02,
            FramePayload::Set(client_set_payload),
        )
    );

    write_next_frame(&mut master_io, &mut master)?;
    let client_set_ack = read_next_frame(&mut device_io, &mut client)?;
    assert_eq!(
        client_set_ack,
        Frame::new(Address::Unicast(0x02), 0x01, FramePayload::Ack(0x01))
    );

    Ok(())
}
