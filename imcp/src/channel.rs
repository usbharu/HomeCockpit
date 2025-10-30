use crate::frame::Frame;

pub trait Sender {
    async fn send(&mut self, frame: Frame);
}

pub trait Receiver {
    async fn receive(&mut self) -> Frame;
}

pub trait SyncSender<E> {
    fn send(&mut self, frame: Frame) -> Result<(), E>;
}

pub trait SyncReceiver<E> {
    fn receive(&mut self) -> Result<Frame, E>;
}
