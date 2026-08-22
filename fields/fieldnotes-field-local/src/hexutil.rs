//! Lowercase hexadecimal rendering, shared by the scope digest and the
//! artifact detection-aid digest so both match A1's digest spelling exactly
//! the same way.

/// Renders bytes as lowercase hexadecimal.
#[must_use]
pub(crate) fn to_hex(bytes: &[u8]) -> String {
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
    use super::to_hex;

    #[test]
    fn renders_lowercase_hex() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff, 0xa5]), "000fffa5");
    }
}
