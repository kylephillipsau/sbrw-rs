// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Ported from WorldUnitedNFS/freeroam (server.go, client.go).

//! The free-roam server's handshake. A client says hello with a 58-byte
//! packet whose third byte is 0x06, carrying its tick at bytes 52..54; the
//! server answers with 11 bytes: its sequence number, 0x01, its own tick, the
//! difference between the two ticks, and four constant bytes. Ticks are
//! milliseconds truncated to 16 bits, big-endian throughout.

/// The client's tick from a hello packet, or `None` if this isn't one
/// (`Server.RunPacketRead`).
pub fn hello_tick(packet: &[u8]) -> Option<u16> {
    (packet.len() == 58 && packet[2] == 0x06).then(|| u16::from_be_bytes([packet[52], packet[53]]))
}

/// `int16(InitialTick - getServerTick())`, as `newClient` keeps it.
pub fn tick_diff(client_tick: u16, server_tick: u16) -> i16 {
    client_tick.wrapping_sub(server_tick) as i16
}

/// `Client.replyHandshake`.
pub fn handshake_reply(seq: u16, server_tick: u16, tick_diff: i16) -> [u8; 11] {
    let mut out = [0; 11];
    out[0..2].copy_from_slice(&seq.to_be_bytes());
    out[2] = 0x01;
    out[3..5].copy_from_slice(&server_tick.to_be_bytes());
    out[5..7].copy_from_slice(&tick_diff.to_be_bytes());
    out[7..11].copy_from_slice(&[0x49, 0x26, 0x03, 0x01]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        packet: String,
        reply: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        cases: Vec<Case>,
    }

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn matches_the_go_server() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/freeroam_handshake.json");
        let text = std::fs::read_to_string(path).expect("run reference/freeroam/handshake_vectors.py");
        let vectors: Vectors = serde_json::from_str(&text).unwrap();
        assert!(vectors.cases.iter().any(|c| c.reply.is_some()) && vectors.cases.iter().any(|c| c.reply.is_none()));
        for case in vectors.cases {
            let packet = bytes(&case.packet);
            let Some(reply) = case.reply.as_deref().map(bytes) else {
                assert_eq!(hello_tick(&packet), None, "freeroamd ignored {}", case.packet);
                continue;
            };
            let client_tick = hello_tick(&packet).expect("freeroamd answered it as a hello");
            let server_tick = u16::from_be_bytes([reply[3], reply[4]]);
            // freeroamd reads its clock once for the difference and again for
            // the reply, so the first read can be a millisecond earlier.
            let same_ms = handshake_reply(0, server_tick, tick_diff(client_tick, server_tick));
            let ms_before = handshake_reply(0, server_tick, tick_diff(client_tick, server_tick.wrapping_sub(1)));
            assert!(reply == same_ms || reply == ms_before, "{} -> {}", case.packet, case.reply.unwrap());
        }
    }

    #[test]
    fn recognises_a_hello_by_length_and_type() {
        let mut hello = [0u8; 58];
        hello[2] = 0x06;
        hello[52..54].copy_from_slice(&0xBEEFu16.to_be_bytes());
        assert_eq!(hello_tick(&hello), Some(0xBEEF));
        assert_eq!(hello_tick(&hello[..57]), None);
        hello[2] = 0x07;
        assert_eq!(hello_tick(&hello), None);
    }

    #[test]
    fn replies_with_sequence_ticks_and_the_constant_tail() {
        // The client's tick is behind the server's across the 16-bit wrap.
        let diff = tick_diff(0xFFF0, 0x0010);
        assert_eq!(diff, -32);
        assert_eq!(
            handshake_reply(0, 0x0010, diff),
            [0x00, 0x00, 0x01, 0x00, 0x10, 0xFF, 0xE0, 0x49, 0x26, 0x03, 0x01]
        );
    }
}
