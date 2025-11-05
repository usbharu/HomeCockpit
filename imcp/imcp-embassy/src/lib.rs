#![cfg_attr(not(test), no_std)]

use embassy_sync::{
    blocking_mutex::raw::RawMutex,
    channel::{TryReceiveError, TrySendError},
};
use imcp::{
    channel::{Receiver, Sender, SyncReceiver, SyncSender},
    frame::Frame,
};

pub struct EmbassySender<'ch, M: RawMutex, const N: usize> {
    sender: embassy_sync::channel::Sender<'ch, M, Frame, N>,
}

pub struct EmbassyReceiver<'ch, M: RawMutex, const N: usize> {
    receiver: embassy_sync::channel::Receiver<'ch, M, Frame, N>,
}

pub fn new<'ch, M: RawMutex, const N: usize>(
    sender: embassy_sync::channel::Sender<'ch, M, Frame, N>,
    receiver: embassy_sync::channel::Receiver<'ch, M, Frame, N>,
) -> (EmbassySender<'ch, M, N>, EmbassyReceiver<'ch, M, N>) {
    (EmbassySender { sender }, EmbassyReceiver { receiver })
}

impl<'ch, M: RawMutex, const N: usize> Sender for EmbassySender<'ch, M, N> {
    async fn send(&mut self, frame: Frame) {
        self.sender.send(frame).await;
    }
}

impl<'ch, M: RawMutex, const N: usize> Receiver for EmbassyReceiver<'ch, M, N> {
    async fn receive(&mut self) -> Frame {
        self.receiver.receive().await
    }
}

impl<'ch, M: RawMutex, const N: usize> SyncSender<TrySendError<Frame>>
    for EmbassySender<'ch, M, N>
{
    fn send(&mut self, frame: Frame) -> Result<(), TrySendError<Frame>> {
        self.sender.try_send(frame)
    }
}

impl<'ch, M: RawMutex, const N: usize> SyncReceiver<TryReceiveError>
    for EmbassyReceiver<'ch, M, N>
{
    fn receive(&mut self) -> Result<Frame, TryReceiveError> {
        self.receiver.try_receive()
    }
}

#[cfg(test)]
mod tests {
    use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
    use imcp::{error::*, frame::*, parser::FrameParser, *};

    use crate::{EmbassyReceiver, EmbassySender};
    use futures::executor::block_on;

    #[test]
    fn test_read_tick_ignore_broadcast_ack() {
        block_on(async {
            static FRAME_CHANNEL: Channel<CriticalSectionRawMutex, Frame, 5> = Channel::new();

            let mut rx_buffer = [0u8; 128];
            let mut parser_frame_buffer = [0u8; 128];
            let frame_parser = FrameParser::new(&mut rx_buffer, &mut parser_frame_buffer);

            let sender = EmbassySender {
                sender: FRAME_CHANNEL.sender(),
            };

            let receiver = EmbassyReceiver {
                receiver: FRAME_CHANNEL.receiver(),
            };

            let mut imcp = Imcp::new(
                receiver,
                sender,
                0x02,
                None,
                frame_parser,
                NodeType::Client(ClientState::Ready),
            );

            let data: &[u8] = &[
                SOF,
                0x02,
                0x01,
                0x02,
                0x01,
                0x00,
                ESC,
                ESC_XOR ^ EOF,
                ESC,
                ESC_XOR ^ EOF,
                EOF,
            ];

            let result = imcp.read_tick(data);

            let _frame = result.await.unwrap().unwrap();
        });
    }

    #[test]
    fn test_read_tick_unexpected_ack() {
        block_on(async {
            static FRAME_CHANNEL: Channel<CriticalSectionRawMutex, Frame, 5> = Channel::new();

            let mut rx_buffer = [0u8; 128];
            let mut parser_frame_buffer = [0u8; 128];
            let frame_parser = FrameParser::new(&mut rx_buffer, &mut parser_frame_buffer);

            let sender = EmbassySender {
                sender: FRAME_CHANNEL.sender(),
            };

            let receiver = EmbassyReceiver {
                receiver: FRAME_CHANNEL.receiver(),
            };

            let mut imcp = Imcp::new(
                receiver,
                sender,
                0x02,
                None,
                frame_parser,
                NodeType::Client(ClientState::Ready),
            );
            let data: &[u8] = &[SOF, 0x02, 0x01, 0x02, 0x01, 0x00, 0x01, 0x01, EOF];

            let result = imcp.read_tick(data);

            assert_eq!(
                Err(ImcpError::ProtocolError(ProtocolError::UnexpectedAck)),
                result.await
            )
        });
    }
    #[test]
    fn test_read_tick_other_set_address_client() {
        block_on(async {
            static FRAME_CHANNEL: Channel<CriticalSectionRawMutex, Frame, 5> = Channel::new();

            let mut rx_buffer = [0u8; 128];
            let mut parser_frame_buffer = [0u8; 128];
            let frame_parser = FrameParser::new(&mut rx_buffer, &mut parser_frame_buffer);

            let sender = EmbassySender {
                sender: FRAME_CHANNEL.sender(),
            };

            let receiver = EmbassyReceiver {
                receiver: FRAME_CHANNEL.receiver(),
            };

            let mut imcp = Imcp::new(
                receiver,
                sender,
                0x02,
                None,
                frame_parser,
                NodeType::Client(ClientState::NotReady),
            );

            let data: &[u8] = &[
                SOF, 0x00, 0x01, 0x04, 0x05, 0x00, 0x02, 12, 0x00, 0x00, 0x00, 0x0e, EOF,
            ];

            imcp.send_join(11).await;

            let result = imcp.read_tick(data);

            assert_eq!(Ok(None), result.await)
        })
    }

    #[test]
    fn test_read_tick_set_address_client() {
        block_on(async {
            static FRAME_CHANNEL: Channel<CriticalSectionRawMutex, Frame, 5> = Channel::new();

            let mut rx_buffer = [0u8; 128];
            let mut parser_frame_buffer = [0u8; 128];
            let frame_parser = FrameParser::new(&mut rx_buffer, &mut parser_frame_buffer);

            let sender = EmbassySender {
                sender: FRAME_CHANNEL.sender(),
            };

            let receiver = EmbassyReceiver {
                receiver: FRAME_CHANNEL.receiver(),
            };

            let mut imcp = Imcp::new(
                receiver,
                sender,
                0x00,
                None,
                frame_parser,
                NodeType::Client(ClientState::NotReady),
            );

            let data: &[u8] = &[
                SOF, 0x00, 0x01, 0x04, 0x05, 0x00, 0x02, 12, 0x00, 0x00, 0x00, 0x0e, EOF,
            ];

            imcp.send_join(12).await;

            {
                imcp.write_tick().await.unwrap();
            }

            let result = imcp.read_tick(data);

            {
                let frame = result.await.unwrap().unwrap();

                assert_eq!(frame.payload().frame_type(), FrameType::SetAddress);
                println!("{:?}", frame);
            }
            assert_eq!(imcp.address(), 2);
        })
    }
}
