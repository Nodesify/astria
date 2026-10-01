// frame: Bolt's chunked message framing — every message travels as one or
// more size-prefixed chunks (u16 big-endian, max 65_535 bytes each),
// terminated by a zero-length chunk. Shared by the client and the mock
// server in tests so both sides of the wire speak identical framing.

use std::io::{Read, Write};

/// Maximum bytes in one chunk.
const CHUNK_SIZE: usize = 65_535;

/// Upper bound on one deframed message. Bolt has no message-size limit of
/// its own, so a peer streaming chunks forever would otherwise grow this
/// buffer without end; 256 MiB is orders of magnitude beyond any real
/// response this client receives (graph records for one export run).
/// Shrunk in tests so the cap's own unit test stays cheap.
#[cfg(not(test))]
const MAX_MESSAGE_BYTES: usize = 256 * 1024 * 1024;
/// Shrunk in tests so the cap's own unit test stays cheap — but it must
/// stay above the large-message round-trip test's 65 KiB payload.
#[cfg(test)]
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

pub fn write_message<W: Write>(stream: &mut W, message: &[u8]) -> std::io::Result<()> {
    if message.is_empty() {
        stream.write_all(&0u16.to_be_bytes())?;
    } else {
        for chunk in message.chunks(CHUNK_SIZE) {
            stream.write_all(&(chunk.len() as u16).to_be_bytes())?;
            stream.write_all(chunk)?;
        }
        stream.write_all(&0u16.to_be_bytes())?;
    }
    stream.flush()
}

/// Read one complete message (all chunks deframed and concatenated).
pub fn read_message<R: Read>(stream: &mut R) -> std::io::Result<Vec<u8>> {
    let mut message = Vec::new();
    loop {
        let mut size_buf = [0u8; 2];
        stream.read_exact(&mut size_buf)?;
        let size = u16::from_be_bytes(size_buf) as usize;
        if size == 0 {
            return Ok(message);
        }
        if message.len() + size > MAX_MESSAGE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "bolt message exceeds {} MiB before its terminating chunk",
                    MAX_MESSAGE_BYTES / (1024 * 1024)
                ),
            ));
        }
        let start = message.len();
        message.resize(start + size, 0);
        stream.read_exact(&mut message[start..])?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_messages_split_into_chunks_and_rejoin() {
        let message = vec![0xABu8; CHUNK_SIZE + 100];
        let mut wire = Vec::new();
        write_message(&mut wire, &message).unwrap();
        // Two full chunks + remainder + terminator.
        assert_eq!(wire.len(), 2 + CHUNK_SIZE + 2 + 100 + 2);
        let mut cursor = std::io::Cursor::new(wire);
        let read_back = read_message(&mut cursor).unwrap();
        assert_eq!(read_back, message);
    }

    #[test]
    fn message_over_the_cap_errors_instead_of_growing_forever() {
        // Chunks without a terminator: a hostile or broken peer streaming
        // past the cap must produce InvalidData, not unbounded memory
        // growth. MAX_MESSAGE_BYTES is shrunk under cfg(test) (see the
        // const) so this needs only a couple of chunks.
        let chunks_needed = (MAX_MESSAGE_BYTES / CHUNK_SIZE) + 2;
        let mut wire = Vec::new();
        for _ in 0..chunks_needed {
            wire.extend_from_slice(&(CHUNK_SIZE as u16).to_be_bytes());
            wire.extend(std::iter::repeat(0xABu8).take(CHUNK_SIZE));
        }
        let mut cursor = std::io::Cursor::new(wire);
        let err = read_message(&mut cursor).expect_err("oversized message must error");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }
}
