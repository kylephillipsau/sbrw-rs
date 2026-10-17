// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Ported from WorldUnitedNFS/freeroam (binary/better_bitstream.go,
// carstate/packetdata.go, carstate/ground.go, carstate/air.go).

//! Car states: the 0x12 subpacket, where the game packs its car's position,
//! velocity and orientation into bit fields. Checked against the Go decoder
//! (`fixtures/vectors/freeroam_carstate.json`).
//!
//! - A 16-bit sim time, 12 header bits whose fifth is "on the ground", then
//!   the fields of a ground or an air state, most significant bit first.
//! - Each field is a quantised float. A raw value at or above the field's
//!   threshold takes one more bit, so the encoding has variable length.
//! - Reading past the end gives zero bits, so every state of two or more
//!   bytes decodes: the Go decoder has no error it can reach.
//!
//! Only the position matters to the server, for who sees whom. The rest is
//! decoded for logs and tests. The Go server's rotation, used only by its
//! live map, is left out.

/// `binary.Bitstream`: a 32-bit buffer over the bytes, refilled from the
/// current position when it runs short.
struct Bitstream<'a> {
    data: &'a [u8],
    data_offset: usize,
    bits_into: u32,
    buffer_used: u32,
    buffer: u32,
}

impl<'a> Bitstream<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bitstream { data, data_offset: 0, bits_into: 0, buffer_used: 0, buffer: 0 }
    }

    fn count_bits_left(&self) -> u32 {
        (self.data.len() - self.data_offset) as u32 * 8 - self.bits_into
    }

    /// `ReadBits`: the next `count` bits, or as many as are left. Go's errors
    /// can't happen: `count` is at most 17 here, and the offset never passes
    /// the end because `count` is clamped first.
    fn read_bits(&mut self, count: u32) -> u32 {
        debug_assert!(count <= 32);
        let count = count.min(self.count_bits_left());
        if count == 0 {
            return 0;
        }
        if self.buffer_used < count {
            self.fill_buffer();
        }
        let bits = (self.buffer >> (32 - count)) & (u32::MAX >> (32 - count));
        self.update_buffer(count);
        let count = count + self.bits_into;
        self.data_offset += (count / 8) as usize;
        self.bits_into = count % 8;
        bits
    }

    fn read_bit(&mut self) -> bool {
        self.read_bits(1) == 1
    }

    fn fill_buffer(&mut self) {
        self.buffer_used = self.count_bits_left().min(32);
        let rest = &self.data[self.data_offset..];
        let into = self.bits_into;
        let byte = |i: usize, shift: u32| u32::from(rest[i]) << shift;
        self.buffer = match rest.len() {
            0 => self.buffer,
            1 => byte(0, 24 + into),
            2 => byte(0, 24 + into) | byte(1, 16 + into),
            3 => byte(0, 24 + into) | byte(1, 16 + into) | byte(2, 8 + into),
            4 => byte(0, 24 + into) | byte(1, 16 + into) | byte(2, 8 + into) | byte(3, into),
            _ => {
                byte(0, 24 + into)
                    | byte(1, 16 + into)
                    | byte(2, 8 + into)
                    | byte(3, into)
                    | (u32::from(rest[4]) >> (8 - into))
            }
        };
    }

    fn update_buffer(&mut self, count: u32) {
        let count = count.min(self.buffer_used);
        // Go's `<<=` of 32 or more gives zero.
        self.buffer = self.buffer.checked_shl(count).unwrap_or(0);
        self.buffer_used -= count;
    }
}

/// `PacketReader.DecodeFloat`.
fn decode_float(bits: &mut Bitstream, num_bits: u32, max_value: u32, add1: f64, multiply: f64, add2: f64) -> f64 {
    let mut raw = bits.read_bits(num_bits);
    if raw >= max_value {
        let has_data_left = u32::from(bits.read_bit());
        raw = has_data_left.wrapping_add(raw.wrapping_mul(2)).wrapping_sub(max_value);
    }
    (f64::from(raw) + add1) * multiply + add2
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Ground {
        /// x, y, z, w.
        orientation: [f64; 4],
        front_wheels: f64,
        rear_wheels: f64,
        /// `ActiveEffectFlags`: lights and the like, 13 bits.
        effects: u32,
    },
    Air {
        yaw: f64,
        pitch: f64,
        roll: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CarState {
    pub sim_time: u16,
    pub pos: Vec3,
    pub lin_vel: Vec3,
    pub ang_vel: Vec3,
    pub kind: Kind,
}

impl CarState {
    pub fn on_ground(&self) -> bool {
        matches!(self.kind, Kind::Ground { .. })
    }
}

/// `PacketReader.Decode`, then `GroundPacket.Decode` or `AirPacket.Decode`.
/// The constants are the Go code's, which are the game's own.
#[allow(clippy::approx_constant)]
pub fn decode(packet: &[u8]) -> CarState {
    let mut bits = Bitstream::new(packet);
    let sim_time = bits.read_bits(16) as u16;
    bits.read_bits(2);
    bits.read_bit();
    bits.read_bit();
    let ground = bits.read_bit();
    bits.read_bit();
    bits.read_bits(6);
    let b = &mut bits;
    if ground {
        let pos_y = decode_float(b, 17, 62144, 0.5, 0.039999999, -5000.0);
        let pos_z = decode_float(b, 11, 96, 0.5, 0.12774999, -112.0);
        let pos_x = decode_float(b, 17, 62144, 0.5, 0.059999999, 0.0);
        let lin_vel_y = decode_float(b, 14, 0x31E0, 0.5, 0.016666668, -166.66667);
        let lin_vel_z = decode_float(b, 10, 0x350, 0.5, 0.1388889, -83.333336);
        let lin_vel_x = decode_float(b, 14, 0x31E0, 0.5, 0.016666668, -166.66667);
        let orientation_y = decode_float(b, 8, 1, 0.5, 0.0039138943, -1.0);
        let orientation_z = decode_float(b, 9, 0xE0, 0.5, 0.0024999999, -1.0);
        let orientation_x = decode_float(b, 8, 1, 0.5, 0.0039138943, -1.0);
        let orientation_w = decode_float(b, 9, 0xE0, 0.5, 0.0024999999, -1.0);
        let ang_vel_y = decode_float(b, 8, 0xD3, 0.5, 0.12524623, -18.849556);
        let ang_vel_z = decode_float(b, 8, 0xD3, 0.5, 0.12524623, -18.849556);
        let ang_vel_x = decode_float(b, 8, 0xD3, 0.5, 0.12524623, -18.849556);
        let front_wheels = decode_float(b, 6, 0x1B, 0.5, 0.021980198, -1.11);
        let rear_wheels = decode_float(b, 6, 0x1B, 0.5, 0.021980198, -1.11);
        let effects = b.read_bits(13);
        CarState {
            sim_time,
            pos: Vec3 { x: pos_x, y: -pos_y, z: pos_z },
            lin_vel: Vec3 { x: lin_vel_x, y: -lin_vel_y, z: lin_vel_z },
            ang_vel: Vec3 { x: ang_vel_x, y: -ang_vel_y, z: ang_vel_z },
            kind: Kind::Ground {
                orientation: [orientation_x, orientation_y, orientation_z, orientation_w],
                front_wheels,
                rear_wheels,
                effects,
            },
        }
    } else {
        let yaw = decode_float(b, 9, 0xE0, 0.5, 0.007853982, -3.1415927);
        let pitch = decode_float(b, 8, 0x70, 0.5, 0.007853982, -1.5707964);
        let roll = decode_float(b, 9, 0xE0, 0.5, 0.007853982, -3.1415927);
        let pos_y = decode_float(b, 17, 62144, 0.5, 0.15000001, -15000.0);
        let pos_z = decode_float(b, 11, 96, 0.5, 0.639999999, -512.0);
        let pos_x = decode_float(b, 17, 62144, 0.5, 0.15000001, -15000.0);
        let lin_vel_y = decode_float(b, 9, 0x7B, 0.5, 0.30829942, -138.8889);
        let lin_vel_z = decode_float(b, 9, 0x7B, 0.5, 0.30829942, -138.8889);
        let lin_vel_x = decode_float(b, 9, 0x7B, 0.5, 0.30829942, -138.8889);
        let ang_vel_y = decode_float(b, 8, 0xD3, 0.5, 0.12524623, -18.849556);
        let ang_vel_z = decode_float(b, 8, 0xD3, 0.5, 0.12524623, -18.849556);
        let ang_vel_x = decode_float(b, 8, 0xD3, 0.5, 0.12524623, -18.849556);
        CarState {
            sim_time,
            pos: Vec3 { x: pos_x, y: -pos_y, z: pos_z },
            lin_vel: Vec3 { x: lin_vel_x, y: -lin_vel_y, z: lin_vel_z },
            ang_vel: Vec3 { x: ang_vel_x, y: -ang_vel_y, z: ang_vel_z },
            kind: Kind::Air { yaw, pitch, roll },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        packet: String,
        error: Option<String>,
        sim_time: u16,
        ground: bool,
        pos: [String; 3],
        lin_vel: [String; 3],
        ang_vel: [String; 3],
        orientation: Option<[String; 4]>,
        wheels: Option<[String; 2]>,
        effects: Option<u32>,
        yaw_pitch_roll: Option<[String; 3]>,
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        cases: Vec<Case>,
    }

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
    }

    fn bits(v: f64) -> String {
        format!("{:016x}", v.to_bits())
    }

    fn vec_bits(v: Vec3) -> [String; 3] {
        [bits(v.x), bits(v.y), bits(v.z)]
    }

    #[test]
    fn decodes_as_the_go_server_does() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/freeroam_carstate.json");
        let text = std::fs::read_to_string(path).expect("run reference/freeroam/carstate_vectors.py");
        let vectors: Vectors = serde_json::from_str(&text).unwrap();
        assert!(vectors.cases.len() > 1000);
        assert!(vectors.cases.iter().any(|c| c.ground) && vectors.cases.iter().any(|c| !c.ground));
        for case in vectors.cases {
            assert_eq!(case.error, None, "Go failed to decode {}", case.packet);
            let state = decode(&bytes(&case.packet));
            let p = &case.packet;
            assert_eq!(state.sim_time, case.sim_time, "{p}");
            assert_eq!(state.on_ground(), case.ground, "{p}");
            assert_eq!(vec_bits(state.pos), case.pos, "{p}");
            assert_eq!(vec_bits(state.lin_vel), case.lin_vel, "{p}");
            assert_eq!(vec_bits(state.ang_vel), case.ang_vel, "{p}");
            match state.kind {
                Kind::Ground { orientation, front_wheels, rear_wheels, effects } => {
                    assert_eq!(Some(orientation.map(bits)), case.orientation, "{p}");
                    assert_eq!(Some([bits(front_wheels), bits(rear_wheels)]), case.wheels, "{p}");
                    assert_eq!(Some(effects), case.effects, "{p}");
                }
                Kind::Air { yaw, pitch, roll } => {
                    assert_eq!(Some([bits(yaw), bits(pitch), bits(roll)]), case.yaw_pitch_roll, "{p}");
                }
            }
        }
    }

    #[test]
    fn reads_bits_most_significant_first_and_stops_at_the_end() {
        let mut bits = Bitstream::new(&[0b1011_0011, 0xFF]);
        assert_eq!(bits.read_bits(3), 0b101);
        assert_eq!(bits.read_bits(9), 0b1_0011_1111);
        // Four bits are left; asking for more gives those four.
        assert_eq!(bits.read_bits(17), 0b1111);
        assert_eq!(bits.read_bits(5), 0);
        assert!(!bits.read_bit());
    }
}
