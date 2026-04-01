use core::fmt;

use imcp::frame::Frame;
use tokio::sync::mpsc::{Receiver, Sender};

pub struct TokioSender {
    sender: Sender<Frame>,
}

pub struct TokioReceiver {
    receiver: Receiver<Frame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokioChannelError {
    Closed,
}

impl fmt::Display for TokioChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokioChannelError::Closed => write!(f, "channel closed"),
        }
    }
}

impl imcp::channel::Sender for TokioSender {
    type Error = TokioChannelError;

    async fn send(&mut self, frame: Frame) -> Result<(), Self::Error> {
        self.sender
            .send(frame)
            .await
            .map_err(|_| TokioChannelError::Closed)
    }
}

impl TokioSender {
    pub async fn try_send(
        &mut self,
        frame: Frame,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<Frame>> {
        self.sender.send(frame).await
    }
}

impl imcp::channel::Receiver for TokioReceiver {
    type Error = TokioChannelError;

    async fn receive(&mut self) -> Result<Frame, Self::Error> {
        self.receiver.recv().await.ok_or(TokioChannelError::Closed)
    }
}

impl TokioReceiver {
    pub async fn try_receive(&mut self) -> Option<Frame> {
        self.receiver.recv().await
    }
}
