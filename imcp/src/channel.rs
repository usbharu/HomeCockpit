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

    /// Take a protocol response that must be sent before retrying a pending
    /// reliable Set. Receivers without a priority channel return None.
    fn try_receive_urgent(&mut self) -> Result<Option<Frame>, Self::Error> {
        Ok(None)
    }
}

pub trait SyncSender<E> {
    fn send(&mut self, frame: Frame) -> Result<(), E>;
}

pub trait SyncReceiver<E> {
    fn receive(&mut self) -> Result<Frame, E>;
}
