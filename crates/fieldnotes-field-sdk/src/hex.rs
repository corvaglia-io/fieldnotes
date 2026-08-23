//! Lowercase hexadecimal rendering, matching A1's digest spelling.
//!
//! Shared by [`crate::scope`] and [`crate::stage`] so both render a SHA-256
//! digest exactly the same way as every other hash domain in the workspace.

/// Renders `bytes` as lowercase hexadecimal, two digits per byte.
#[must_use]
pub fn to_lower_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        hex.push(hex_digit(byte >> 4));
        hex.push(hex_digit(byte & 0x0f));
    }
    hex
}

fn hex_digit(nibble: u8) -> char {
    char::from_digit(u32::from(nibble), 16).unwrap_or('0')
}

#[cfg(test)]
mod tests {
    use super::to_lower_hex;

    #[test]
    fn renders_lowercase_hex() {
        assert_eq!(to_lower_hex(&[0x00, 0x0f, 0xff, 0xa5]), "000fffa5");
    }

    #[test]
    fn renders_an_empty_slice_as_an_empty_string() {
        assert_eq!(to_lower_hex(&[]), "");
    }
}
