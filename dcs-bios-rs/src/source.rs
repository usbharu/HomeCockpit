use std::vec::Vec;

use crate::error::Error;

pub trait Source {
    fn setup(&self) -> Result<(), Error>;
    fn read(&mut self) -> Result<Option<Vec<u8>>, Error>;
}
