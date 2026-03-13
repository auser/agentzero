//! Fuzz target for WebSocket frame header parsing.
//!
//! Implements the RFC 6455 frame header parser inline to verify that no
//! panic occurs on malformed frames. This mirrors the frame parsing done
//! by tokio-tungstenite in the gateway's WebSocket handler.
//!
//! Frame layout (RFC 6455 §5.2):
//!   Byte 0: FIN(1) RSV1(1) RSV2(1) RSV3(1) Opcode(4)
//!   Byte 1: MASK(1) Payload-len(7)
//!     If payload-len == 126: next 2 bytes are the actual length (u16 BE)
//!     If payload-len == 127: next 8 bytes are the actual length (u64 BE)
//!   If MASK==1: 4 masking-key bytes follow the length
//!   Then: payload_len bytes of (masked) payload

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    parse_ws_frame(data);
});

/// Returns `Some((opcode, payload))` on a valid frame, `None` on truncated/invalid input.
/// Must never panic.
fn parse_ws_frame(data: &[u8]) -> Option<(u8, Vec<u8>)> {
    if data.len() < 2 {
        return None;
    }

    let byte0 = data[0];
    let byte1 = data[1];

    let _fin = (byte0 & 0x80) != 0;
    let opcode = byte0 & 0x0f;
    let masked = (byte1 & 0x80) != 0;
    let payload_len_7 = (byte1 & 0x7f) as usize;

    let mut cursor = 2usize;

    let payload_len: usize = match payload_len_7 {
        126 => {
            if data.len() < cursor + 2 {
                return None;
            }
            let len = u16::from_be_bytes([data[cursor], data[cursor + 1]]) as usize;
            cursor += 2;
            len
        }
        127 => {
            if data.len() < cursor + 8 {
                return None;
            }
            let len = u64::from_be_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
                data[cursor + 4],
                data[cursor + 5],
                data[cursor + 6],
                data[cursor + 7],
            ]) as usize;
            cursor += 8;
            // Reject frames that would exceed a 16 MB safety limit.
            if len > 16 * 1024 * 1024 {
                return None;
            }
            len
        }
        n => n,
    };

    let mask: Option<[u8; 4]> = if masked {
        if data.len() < cursor + 4 {
            return None;
        }
        let key = [
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ];
        cursor += 4;
        Some(key)
    } else {
        None
    };

    if data.len() < cursor + payload_len {
        return None;
    }

    let raw_payload = &data[cursor..cursor + payload_len];
    let payload: Vec<u8> = match mask {
        Some(key) => raw_payload
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ key[i % 4])
            .collect(),
        None => raw_payload.to_vec(),
    };

    Some((opcode, payload))
}
