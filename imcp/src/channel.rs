use crate::frame::Frame;

#[allow(async_fn_in_trait)]
pub trait Sender {
    type Error;

    async fn send(&mut self, frame: Frame) -> Result<(), Self::Error>;
}

#[allow(async_fn_in_trait)]
pub trait Receiver {
    type Error;

    async fn receive(&mut self) -> Result<Frame, Self::Error>;
}

pub trait SyncSender<E> {
    fn send(&mut self, frame: Frame) -> Result<(), E>;
}

pub trait SyncReceiver<E> {
    fn receive(&mut self) -> Result<Frame, E>;
}
