#![cfg(unix)]

use std::{
    ffi::CStr,
    fs::File,
    io::{self, Read, Write},
    os::fd::{AsRawFd, FromRawFd, RawFd},
    path::{Path, PathBuf},
    ptr,
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Readable,
    Disconnected,
    Timeout,
}

pub struct Pty {
    master: File,
    slave_path: PathBuf,
}

impl Pty {
    pub fn open() -> io::Result<Self> {
        let mut master_fd: RawFd = -1;
        let mut slave_fd: RawFd = -1;
        // SAFETY: openpty initializes both descriptors on success. They are immediately
        // wrapped in File values so every successful descriptor has exactly one owner.
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

        // SAFETY: both descriptors were returned by the successful openpty call above.
        let master = unsafe { File::from_raw_fd(master_fd) };
        // SAFETY: slave_fd is independently owned and valid after openpty succeeds.
        let slave = unsafe { File::from_raw_fd(slave_fd) };
        set_raw_mode(slave.as_raw_fd())?;
        let slave_path = tty_path(slave.as_raw_fd())?;
        set_nonblocking(master.as_raw_fd())?;
        drop(slave);

        Ok(Self { master, slave_path })
    }

    pub fn slave_path(&self) -> &Path {
        &self.slave_path
    }

    pub fn wait_readable(&self, timeout: Duration) -> io::Result<Readiness> {
        let timeout_ms = i32::try_from(timeout.as_millis().min(i32::MAX as u128))
            .map_err(|_| io::Error::other("PTY poll timeout is out of range"))?;
        let mut poll_fd = libc::pollfd {
            fd: self.master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };

        loop {
            // SAFETY: poll_fd points to one initialized pollfd for the duration of this call.
            let result = unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) };
            if result == -1 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(error);
            }
            if result == 0 {
                return Ok(Readiness::Timeout);
            }
            if poll_fd.revents & libc::POLLIN != 0 {
                return Ok(Readiness::Readable);
            }
            if poll_fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                return Ok(Readiness::Disconnected);
            }
        }
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.master.read(buffer)
    }

    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)?;
        self.master.flush()
    }
}

fn set_raw_mode(fd: RawFd) -> io::Result<()> {
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: termios points to writable storage and fd is a live PTY descriptor.
    if unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: tcgetattr succeeded and initialized termios.
    let mut termios = unsafe { termios.assume_init() };
    // SAFETY: termios is an initialized termios structure.
    unsafe { libc::cfmakeraw(&mut termios) };
    // SAFETY: fd is valid and termios remains alive for this call.
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: fcntl only inspects flags for the live descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is valid and flags came from F_GETFL for the same descriptor.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn tty_path(fd: RawFd) -> io::Result<PathBuf> {
    let mut buffer = [0 as libc::c_char; 1024];
    // SAFETY: buffer is writable, its length is correct, and fd is a live PTY slave.
    let result = unsafe { libc::ttyname_r(fd, buffer.as_mut_ptr(), buffer.len()) };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result));
    }
    // SAFETY: successful ttyname_r writes a NUL-terminated string into buffer.
    let path = unsafe { CStr::from_ptr(buffer.as_ptr()) };
    Ok(PathBuf::from(path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_DEVICE_ID, MockDevice, encode_frame};
    use imcp::{
        frame::{Address, Frame, FramePayload},
        parser::FrameParser,
    };
    use std::fs::OpenOptions;

    #[test]
    fn pty_transfers_bytes_and_can_be_reopened() -> io::Result<()> {
        let mut pty = Pty::open()?;
        let path = pty.slave_path().to_path_buf();

        {
            let mut slave = OpenOptions::new().read(true).write(true).open(&path)?;
            set_raw_mode(slave.as_raw_fd())?;
            pty.write_all(b"first")?;
            let mut received = [0u8; 5];
            slave.read_exact(&mut received)?;
            assert_eq!(&received, b"first");
        }

        let mut slave = OpenOptions::new().read(true).write(true).open(path)?;
        set_raw_mode(slave.as_raw_fd())?;
        slave.write_all(b"again")?;
        assert_eq!(
            pty.wait_readable(Duration::from_secs(1))?,
            Readiness::Readable
        );
        let mut received = [0u8; 5];
        pty.read(&mut received)?;
        assert_eq!(&received, b"again");
        Ok(())
    }

    #[test]
    fn join_and_assignment_cross_the_pty_wire() -> Result<(), String> {
        let mut pty = Pty::open().map_err(|error| error.to_string())?;
        let mut manager = OpenOptions::new()
            .read(true)
            .write(true)
            .open(pty.slave_path())
            .map_err(|error| error.to_string())?;
        set_raw_mode(manager.as_raw_fd()).map_err(|error| error.to_string())?;

        let mut rx_buffer = [0u8; 256];
        let mut parser_buffer = [0u8; 256];
        let mut device = MockDevice::new(DEFAULT_DEVICE_ID, &mut rx_buffer, &mut parser_buffer);
        device.restart().map_err(|error| error.to_string())?;
        let join_wire = device
            .next_wire_frame()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "missing Join frame".to_string())?;
        pty.write_all(&join_wire)
            .map_err(|error| error.to_string())?;

        let mut wire_buffer = [0u8; 256];
        let join_length = manager
            .read(&mut wire_buffer)
            .map_err(|error| error.to_string())?;
        let mut manager_rx = [0u8; 256];
        let mut manager_frame = [0u8; 256];
        let mut parser = FrameParser::new(&mut manager_rx, &mut manager_frame);
        parser
            .write_data(&wire_buffer[..join_length])
            .map_err(|error| format!("{error:?}"))?;
        let join = parser
            .next_frame()
            .ok_or_else(|| "incomplete Join frame".to_string())?
            .map_err(|error| format!("{error:?}"))?;
        let FramePayload::Join(join_id) = join.payload() else {
            return Err("expected Join frame".to_string());
        };

        let assignment = Frame::new(
            Address::Unicast(0x00),
            0x01,
            FramePayload::SetAddress {
                address: 0x02,
                id: *join_id,
            },
        );
        manager
            .write_all(&encode_frame(&assignment).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            pty.wait_readable(Duration::from_secs(1))
                .map_err(|error| error.to_string())?,
            Readiness::Readable
        );
        let assignment_length = pty
            .read(&mut wire_buffer)
            .map_err(|error| error.to_string())?;
        let notices = device
            .receive_bytes(&wire_buffer[..assignment_length])
            .map_err(|error| error.to_string())?;
        assert_eq!(notices, vec![crate::DeviceNotice::AddressAssigned(0x02)]);

        drop(manager);
        let readiness = pty
            .wait_readable(Duration::from_secs(1))
            .map_err(|error| error.to_string())?;
        assert!(matches!(
            readiness,
            Readiness::Readable | Readiness::Disconnected
        ));
        if readiness == Readiness::Readable {
            match pty.read(&mut wire_buffer) {
                Ok(0) => {}
                Err(error) if error.raw_os_error() == Some(libc::EIO) => {}
                result => return Err(format!("expected closed PTY, got {result:?}")),
            }
        }
        device.restart().map_err(|error| error.to_string())?;
        assert!(
            device
                .next_wire_frame()
                .map_err(|error| error.to_string())?
                .is_some()
        );
        Ok(())
    }
}
