use imcp::frame::Frame;
use tokio::sync::mpsc::{Receiver, Sender};

pub struct TokioSender {
    sender: Sender<Frame>,
}

pub struct TokioReceiver {
    receiver: Receiver<Frame>,
}

impl imcp::channel::Sender for TokioSender {
    async fn send(&mut self, frame: Frame) {
        self.sender.send(frame).await.unwrap();
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
    async fn receive(&mut self) -> Frame {
        self.receiver.recv().await.unwrap()
    }
}

impl TokioReceiver {
    pub async fn try_receive(&mut self) -> Option<Frame> {
        self.receiver.recv().await
    }
}
