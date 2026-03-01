// Length-delimited message framing over TCP.
//
// Provides a simple wire format for `message.rs` types: a 4-byte big-endian
// length prefix followed by a JSON-serialized message payload. Both
// `write_message` and `read_message` operate on raw `&[u8]` / `Vec<u8>` —
// the caller handles JSON serialization separately, keeping this module
// format-agnostic.
//
// A `MAX_MESSAGE_SIZE` constant (16 MB) protects against unbounded allocation
// from malformed or malicious length prefixes. State snapshots for mid-game
// join are the largest expected messages.

use std::io::{self, Read, Write};

/// Maximum allowed message size (16 MB). Protects against unbounded allocation
/// from malformed length prefixes. Snapshot payloads are the largest expected
/// messages; 16 MB is generous headroom for even very large world states.
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

/// Write a length-delimited message: 4-byte big-endian length, then payload.
pub fn write_message<W: Write>(writer: &mut W, msg: &[u8]) -> io::Result<()> {
    let len = msg.len();
    if len > MAX_MESSAGE_SIZE as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("message too large: {len} bytes (max {MAX_MESSAGE_SIZE})"),
        ));
    }
    #[expect(clippy::cast_possible_truncation)]
    let len_bytes = (len as u32).to_be_bytes();
    writer.write_all(&len_bytes)?;
    writer.write_all(msg)?;
    writer.flush()?;
    Ok(())
}

/// Read a length-delimited message: 4-byte big-endian length, then payload.
///
/// Returns `UnexpectedEof` if the stream closes cleanly before or during a
/// message. Returns `InvalidData` if the length exceeds `MAX_MESSAGE_SIZE`.
pub fn read_message<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("message too large: {len} bytes (max {MAX_MESSAGE_SIZE})"),
        ));
    }
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn roundtrip_simple_message() {
        let original = b"hello, relay!";
        let mut buf = Vec::new();
        write_message(&mut buf, original).unwrap();

        let mut cursor = Cursor::new(&buf);
        let recovered = read_message(&mut cursor).unwrap();
        assert_eq!(recovered, original);
    }

    #[test]
    fn roundtrip_empty_message() {
        let original = b"";
        let mut buf = Vec::new();
        write_message(&mut buf, original).unwrap();

        let mut cursor = Cursor::new(&buf);
        let recovered = read_message(&mut cursor).unwrap();
        assert_eq!(recovered, original.to_vec());
    }

    #[test]
    fn rejects_oversized_write() {
        let big = vec![0u8; MAX_MESSAGE_SIZE as usize + 1];
        let mut buf = Vec::new();
        let err = write_message(&mut buf, &big).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn rejects_oversized_read() {
        // Craft a length prefix that exceeds MAX_MESSAGE_SIZE.
        let fake_len = (MAX_MESSAGE_SIZE + 1).to_be_bytes();
        let mut cursor = Cursor::new(fake_len.to_vec());
        let err = read_message(&mut cursor).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn read_unexpected_eof() {
        // Only 2 bytes when 4 are needed for the length prefix.
        let mut cursor = Cursor::new(vec![0u8, 1]);
        let err = read_message(&mut cursor).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn multiple_messages_in_sequence() {
        let messages: Vec<&[u8]> = vec![b"first", b"second", b"third"];
        let mut buf = Vec::new();
        for msg in &messages {
            write_message(&mut buf, msg).unwrap();
        }

        let mut cursor = Cursor::new(&buf);
        for expected in &messages {
            let recovered = read_message(&mut cursor).unwrap();
            assert_eq!(recovered, *expected);
        }
    }
}
