use std::io;

#[derive(Debug)]
pub enum Error {
    MemoryMapError(),
    SourceError(),
    CommandError(),
    BufferTooSmall(),
    IoError(io::Error),
}

impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::IoError(value)
    }
}
