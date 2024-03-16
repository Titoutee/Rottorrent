use anyhow::Context;
use bytes::{Buf, BufMut, BytesMut};
use strum_macros::FromRepr;
use tokio_util::codec::{Decoder, Encoder};

#[derive(FromRepr, Debug, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageTag {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Message {
    pub length: u32, // Accounts for tag length (1) + payload length (variable)
    pub tag: MessageTag,
    pub payload: Vec<u8>,
}

pub struct MessageFramer;

const MAX: usize = 1 << 16;

impl Decoder for MessageFramer {
    type Item = Message;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 4 {
            return Ok(None);
        }

        // Read length marker.
        let mut length_bytes = [0u8; 4];
        length_bytes.copy_from_slice(&src[..4]);
        let len = u32::from_be_bytes(length_bytes);
        let len_u = len as usize;

        if len == 0 {
            //this is a heartbeat message
            src.advance(4);
            return self.decode(src);
        }

        if len_u > MAX {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Frame of length {} is too large.", len),
            ));
        }

        if src.len() < 5 {
            //not enough to read the tag
            return Ok(None);
        }

        if src.len() < 4 + len_u {
            // The full string has not yet arrived.
            //
            // We reserve more space in the buffer. This is not strictly
            // necessary, but is a good idea performance-wise.
            src.reserve(4 + len_u - src.len());

            // We inform the Framed that we need more bytes to form the next
            // frame.
            return Ok(None);
        }

        // Use advance to modify src such that it no longer contains
        // this frame.
        let tag = src[4];
        let data = if src.len() > 5 {
            src[5..4 + len_u - 1].to_vec()
        } else {
            vec![]
        };

        src.advance(4 + len_u);
        Ok(Some(Message {
            length: len,
            tag: MessageTag::from_repr(tag)
                .context("Constructing MessageTag from u8 repr")
                .expect("Unknown message tag"),
            payload: data,
        }))
    }
}

impl Encoder<Message> for MessageFramer {
    type Error = std::io::Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // Don't send a string if it is longer than the other end will
        // accept.
        let len = item.payload.len() + 1;
        if len > MAX {
            // Never forget the tag byte (+1)
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Frame of length {} is too large.", len),
            ));
        }

        // Convert the length into a byte array.
        // The cast to u32 cannot overflow due to the length check above.
        let len_slice = u32::to_le_bytes((len as u32).try_into().unwrap());

        // Reserve space in the buffer.
        dst.reserve(4 + len);

        // Write the length and string to the buffer.
        dst.extend_from_slice(&len_slice);
        dst.put_u8(item.tag as u8);
        dst.extend_from_slice(&item.payload);
        Ok(())
    }
}
