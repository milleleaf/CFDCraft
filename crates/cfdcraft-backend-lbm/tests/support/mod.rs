//! A reader of the 3MF packages the backend writes, independent of the writer: it walks the local
//! headers of the archive and takes each member's name, method, time and bytes.

// Each test file uses a part of it.
#![allow(dead_code)]

/// One member of a package.
pub struct Entry {
    pub name: String,
    /// The compression method: 0 is stored.
    pub method: u16,
    /// The modification time and date in the format of the archive.
    pub time: u16,
    pub date: u16,
    pub bytes: Vec<u8>,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
}

/// The members in the order they were written.
pub fn entries(bytes: &[u8]) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut at = 0;
    while bytes.len() >= at + 30 && bytes[at..at + 4] == *b"PK\x03\x04" {
        let flags = u16_at(bytes, at + 6);
        assert_eq!(
            flags & 0x8,
            0,
            "sizes follow the data, which this reader does not follow"
        );
        let size = u32_at(bytes, at + 18);
        let name_len = usize::from(u16_at(bytes, at + 26));
        let extra_len = usize::from(u16_at(bytes, at + 28));
        let start = at + 30 + name_len + extra_len;
        out.push(Entry {
            name: String::from_utf8(bytes[at + 30..at + 30 + name_len].to_vec()).unwrap(),
            method: u16_at(bytes, at + 8),
            time: u16_at(bytes, at + 10),
            date: u16_at(bytes, at + 12),
            bytes: bytes[start..start + size].to_vec(),
        });
        at = start + size;
    }
    out
}

/// The text of the model member of a package.
pub fn model_text(bytes: &[u8]) -> String {
    let all = entries(bytes);
    let model = all
        .iter()
        .find(|entry| entry.name == "3D/3dmodel.model")
        .expect("the package holds a model");
    String::from_utf8(model.bytes.clone()).unwrap()
}
