use std::{fmt::Write as _, io::Write, string::String};

use crate::error::Error;

pub trait CommandSink {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error>;
}

impl<T: Write> CommandSink for T {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error> {
        Write::write_all(self, bytes).map_err(Error::from)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportCommand<'a> {
    identifier: &'a str,
    argument: &'a str,
}

impl<'a> ImportCommand<'a> {
    pub fn new(identifier: &'a str, argument: &'a str) -> Result<Self, Error> {
        validate_identifier(identifier)?;
        validate_argument(argument)?;

        Ok(Self {
            identifier,
            argument,
        })
    }

    pub fn identifier(&self) -> &'a str {
        self.identifier
    }

    pub fn argument(&self) -> &'a str {
        self.argument
    }

    pub fn encoded_len(&self) -> usize {
        self.identifier.len() + 1 + self.argument.len() + 1
    }

    pub fn encode(&self) -> String {
        let mut buffer = String::with_capacity(self.encoded_len());
        self.write_command(&mut buffer).expect("writing to String cannot fail");
        buffer
    }

    pub fn send<S: CommandSink>(&self, sink: &mut S) -> Result<(), Error> {
        sink.write_all(self.identifier.as_bytes())?;
        sink.write_all(b" ")?;
        sink.write_all(self.argument.as_bytes())?;
        sink.write_all(b"\n")
    }

    fn write_command(&self, buffer: &mut String) -> Result<(), Error> {
        write!(buffer, "{} {}\n", self.identifier, self.argument)
            .map_err(|_| Error::BufferTooSmall())
    }
}

fn validate_identifier(identifier: &str) -> Result<(), Error> {
    if identifier.is_empty()
        || identifier
            .chars()
            .any(|ch| ch.is_ascii_whitespace() || ch == '\r' || ch == '\n')
    {
        return Err(Error::CommandError());
    }

    Ok(())
}

fn validate_argument(argument: &str) -> Result<(), Error> {
    if argument.chars().any(|ch| ch == '\r' || ch == '\n') {
        return Err(Error::CommandError());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    #[test]
    fn import_command_encodes_as_plain_text_line() {
        let command = ImportCommand::new("UFC_COMM1_CHANNEL_SELECT", "3").unwrap();

        let encoded = command.encode();

        assert_eq!(encoded, "UFC_COMM1_CHANNEL_SELECT 3\n");
    }

    #[test]
    fn import_command_allows_arguments_with_spaces() {
        let command = ImportCommand::new("IFF_MODE", "AUDIO 1").unwrap();

        let encoded = command.encode();

        assert_eq!(encoded, "IFF_MODE AUDIO 1\n");
    }

    #[test]
    fn import_command_rejects_identifiers_with_whitespace() {
        assert!(ImportCommand::new("BAD IDENT", "1").is_err());
    }

    #[test]
    fn import_command_rejects_arguments_with_newlines() {
        assert!(ImportCommand::new("IDENT", "1\n2").is_err());
    }

    #[test]
    fn import_command_can_be_sent_to_a_sink() {
        let command = ImportCommand::new("MASTER_ARM_SW", "1").unwrap();
        let mut sink = Vec::new();

        command.send(&mut sink).unwrap();

        assert_eq!(sink, b"MASTER_ARM_SW 1\n");
    }
}
