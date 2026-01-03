use std::io::{self, Read, Write};

pub fn read_message<R: Read>(mut reader: R) -> io::Result<serde_json::Value> {
    let mut len_buf = [0u8; 4];
    if let Err(e) = reader.read_exact(&mut len_buf) {
        return Err(e);
    }
    let len = u32::from_ne_bytes(len_buf);

    let mut body = vec![0u8; len as usize];
    reader.read_exact(&mut body)?;

    serde_json::from_slice(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn write_message<W: Write>(mut writer: W, msg: &serde_json::Value) -> io::Result<()> {
    let msg_str = serde_json::to_string(msg)?;
    let len = msg_str.len() as u32;
    let len_buf = len.to_ne_bytes();

    writer.write_all(&len_buf)?;
    writer.write_all(msg_str.as_bytes())?;
    writer.flush()
}
