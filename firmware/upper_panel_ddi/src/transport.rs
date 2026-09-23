use defmt::warn;
use embassy_futures::select::select;
use embassy_rp::{gpio::Output, peripherals::USB, usb::Driver as UsbDriver};
use embassy_usb::{
    class::cdc_acm::{BufferedReceiver, CdcAcmError, Sender},
    driver::EndpointError,
};
use embedded_io_async::{Read, Write};
use imcp_embedded::{ImcpEmbedded, RpUartCarrierSense};
use upper_panel_ddi::packetization::{needs_zero_length_packet, next_packet_len};

pub use upper_panel_ddi::packetization::USB_MAX_PACKET_SIZE;

type UsbDriverType = UsbDriver<'static, USB>;
type UsbSender = Sender<'static, UsbDriverType>;
type UsbReceiver = BufferedReceiver<'static, UsbDriverType>;
type UartTransport = ImcpEmbedded<RpUartCarrierSense, Output<'static>>;

pub enum ReadEvent {
    Data(usize),
    UsbConnected,
    UsbDisconnected,
    UsbIgnoredWhileFaulted,
    UartError,
}

pub enum WriteEvent {
    Sent,
    UsbDisconnected,
    UartError,
}

pub struct ImcpTransport {
    uart: UartTransport,
    usb_sender: UsbSender,
    usb_receiver: UsbReceiver,
    usb_active: bool,
    usb_faulted: bool,
    fault_discard: [u8; USB_MAX_PACKET_SIZE],
}

impl ImcpTransport {
    pub fn new(uart: UartTransport, usb_sender: UsbSender, usb_receiver: UsbReceiver) -> Self {
        Self {
            uart,
            usb_sender,
            usb_receiver,
            usb_active: false,
            usb_faulted: false,
            fault_discard: [0; USB_MAX_PACKET_SIZE],
        }
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> ReadEvent {
        if self.usb_active {
            return self.read_usb(buf).await;
        }

        if self.usb_faulted {
            // wait_connection() only waits for endpoint enablement, which may
            // remain true after a write-side BufferOverflow. Observe an actual
            // disconnect before attempting USB again, while UART stays live.
            return match select(
                self.usb_receiver.read(&mut self.fault_discard),
                self.uart.read(buf),
            )
            .await
            {
                embassy_futures::select::Either::First(Ok(_)) => ReadEvent::UsbIgnoredWhileFaulted,
                embassy_futures::select::Either::First(Err(CdcAcmError::NotConnected)) => {
                    self.usb_faulted = false;
                    ReadEvent::UsbDisconnected
                }
                embassy_futures::select::Either::Second(Ok(size)) => ReadEvent::Data(size),
                embassy_futures::select::Either::Second(Err(error)) => {
                    warn!("uart read error {:?}", error);
                    ReadEvent::UartError
                }
            };
        }

        // Endpoint enablement is the CDC connection signal. DTR, RTS, and line
        // coding are intentionally ignored because IMCP carries raw framed data.
        match select(self.usb_receiver.wait_connection(), self.uart.read(buf)).await {
            embassy_futures::select::Either::First(()) => {
                self.usb_active = true;
                ReadEvent::UsbConnected
            }
            embassy_futures::select::Either::Second(Ok(size)) => ReadEvent::Data(size),
            embassy_futures::select::Either::Second(Err(error)) => {
                warn!("uart read error {:?}", error);
                ReadEvent::UartError
            }
        }
    }

    pub async fn write_frame(&mut self, data: &[u8]) -> WriteEvent {
        if self.usb_active {
            return self.write_usb_frame(data).await;
        }

        if let Err(error) = Write::write_all(&mut self.uart, data).await {
            warn!("uart write error {:?}", error);
            return WriteEvent::UartError;
        }
        if let Err(error) = self.uart.flush().await {
            warn!("uart flush error {:?}", error);
            return WriteEvent::UartError;
        }

        WriteEvent::Sent
    }

    async fn read_usb(&mut self, buf: &mut [u8]) -> ReadEvent {
        match self.usb_receiver.read(buf).await {
            Ok(size) => ReadEvent::Data(size),
            Err(CdcAcmError::NotConnected) => {
                self.usb_active = false;
                self.usb_faulted = false;
                ReadEvent::UsbDisconnected
            }
        }
    }

    async fn write_usb_frame(&mut self, data: &[u8]) -> WriteEvent {
        let packet_size = self.usb_sender.max_packet_size() as usize;
        if packet_size == 0 {
            warn!("usb sender reported zero packet size");
            self.usb_active = false;
            self.usb_faulted = true;
            return WriteEvent::UsbDisconnected;
        }

        let mut offset = 0;
        while offset < data.len() {
            let packet_len = next_packet_len(data.len(), offset, packet_size);
            if let Err(error) = self
                .usb_sender
                .write_packet(&data[offset..offset + packet_len])
                .await
            {
                return self.handle_usb_write_error(error);
            }
            offset += packet_len;
        }

        if needs_zero_length_packet(data.len(), packet_size)
            && let Err(error) = self.usb_sender.write_packet(&[]).await
        {
            return self.handle_usb_write_error(error);
        }

        WriteEvent::Sent
    }

    fn handle_usb_write_error(&mut self, error: EndpointError) -> WriteEvent {
        match error {
            EndpointError::Disabled => {
                self.usb_active = false;
                self.usb_faulted = false;
                WriteEvent::UsbDisconnected
            }
            EndpointError::BufferOverflow => {
                warn!("usb write buffer overflow");
                // A write-side endpoint failure can leave the CDC endpoint
                // unusable while it still reports as connected. Fall back to
                // UART so the IMCP task can restart instead of spinning on
                // repeated USB errors.
                self.usb_active = false;
                self.usb_faulted = true;
                WriteEvent::UsbDisconnected
            }
        }
    }
}
