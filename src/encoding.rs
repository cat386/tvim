//! Кодировки файлов: UTF-8 (с BOM и без) и Windows-1251.
//!
//! Файл, который не читается как UTF-8, считается файлом в кодировке
//! Windows-1251 (старые русские тексты и исходники под Windows) и при
//! записи сохраняется в ней же. Концы строк (`\r\n` или `\n`) тоже
//! запоминаются и сохраняются такими, какими были.

/// Кодировка файла.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Encoding {
    #[default]
    Utf8,
    Utf8Bom,
    Cp1251,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf8Bom => "utf-8-bom",
            Encoding::Cp1251 => "cp1251",
        }
    }

    pub fn from_name(s: &str) -> Option<Encoding> {
        match s.to_lowercase().as_str() {
            "utf-8" | "utf8" => Some(Encoding::Utf8),
            "utf-8-bom" | "utf8bom" | "bom" => Some(Encoding::Utf8Bom),
            "cp1251" | "1251" | "windows-1251" | "win" => Some(Encoding::Cp1251),
            _ => None,
        }
    }
}

/// Символы 0x80–0xBF кодировки Windows-1251 (0xC0–0xFF — это А..я подряд).
const CP1251_HIGH: [u16; 64] = [
    0x0402, 0x0403, 0x201A, 0x0453, 0x201E, 0x2026, 0x2020, 0x2021, 0x20AC, 0x2030, 0x0409, 0x2039, 0x040A, 0x040C, 0x040B, 0x040F, //
    0x0452, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x0098, 0x2122, 0x0459, 0x203A, 0x045A, 0x045C, 0x045B, 0x045F, //
    0x00A0, 0x040E, 0x045E, 0x0408, 0x00A4, 0x0490, 0x00A6, 0x00A7, 0x0401, 0x00A9, 0x0404, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x0407, //
    0x00B0, 0x00B1, 0x0406, 0x0456, 0x0491, 0x00B5, 0x00B6, 0x00B7, 0x0451, 0x2116, 0x0454, 0x00BB, 0x0458, 0x0405, 0x0455, 0x0457,
];

fn cp1251_decode(b: u8) -> char {
    match b {
        0..=0x7F => b as char,
        0x80..=0xBF => char::from_u32(CP1251_HIGH[(b - 0x80) as usize] as u32).unwrap_or('?'),
        _ => char::from_u32(0x0410 + (b - 0xC0) as u32).unwrap_or('?'),
    }
}

fn cp1251_encode(c: char) -> u8 {
    let u = c as u32;
    if u < 0x80 {
        return u as u8;
    }
    if (0x0410..=0x044F).contains(&u) {
        return (u - 0x0410 + 0xC0) as u8;
    }
    CP1251_HIGH.iter().position(|&x| x as u32 == u).map(|p| p as u8 + 0x80).unwrap_or(b'?')
}

/// Прочитанный файл.
pub struct Decoded {
    pub text: String,
    pub encoding: Encoding,
    /// Концы строк `\r\n`.
    pub crlf: bool,
}

/// Распознать кодировку и концы строк.
pub fn decode(bytes: &[u8]) -> Decoded {
    let (text, encoding) = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (String::from_utf8_lossy(rest).into_owned(), Encoding::Utf8Bom)
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) => (s.to_string(), Encoding::Utf8),
            Err(_) => (bytes.iter().map(|&b| cp1251_decode(b)).collect(), Encoding::Cp1251),
        }
    };
    let crlf = text.contains("\r\n");
    Decoded { text, encoding, crlf }
}

/// Записать текст в байты нужной кодировки (`text` — со строками через `\n`).
pub fn encode(text: &str, encoding: Encoding, crlf: bool) -> Vec<u8> {
    let text = if crlf { text.replace('\n', "\r\n") } else { text.to_string() };
    match encoding {
        Encoding::Utf8 => text.into_bytes(),
        Encoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend(text.into_bytes());
            v
        }
        Encoding::Cp1251 => text.chars().map(cp1251_encode).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cp1251_roundtrip() {
        let s = "Привет, Ёлка! №1 «ok» — €";
        let b = encode(s, Encoding::Cp1251, false);
        assert_eq!(b[0], 0xCF); // П
        let d = decode(&b);
        assert_eq!(d.encoding, Encoding::Cp1251);
        assert_eq!(d.text, s);
        let d = decode("a\r\nб".as_bytes());
        assert!(d.crlf);
        assert_eq!(d.encoding, Encoding::Utf8);
        assert_eq!(encode("a\nb", Encoding::Utf8, true), b"a\r\nb");
    }
}
