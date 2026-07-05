fn read_u8(bytes: &[u8], offset: &mut usize) -> Result<u8, ImportError> {
    let value = *bytes
        .get(*offset)
        .ok_or(ImportError::InvalidFormat("unexpected end of data"))?;
    *offset += 1;
    Ok(value)
}

fn read_var_len(bytes: &[u8], offset: &mut usize) -> Result<u64, ImportError> {
    let mut value = 0u64;
    for _ in 0..4 {
        let byte = read_u8(bytes, offset)?;
        value = (value << 7) | u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(ImportError::MalformedTiming(
        "invalid variable length value",
    ))
}

fn be_u16(bytes: &[u8], offset: usize) -> Result<u16, ImportError> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or(ImportError::InvalidFormat("unexpected end of data"))?;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

fn be_u32(bytes: &[u8], offset: usize) -> Result<u32, ImportError> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or(ImportError::InvalidFormat("unexpected end of data"))?;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, ImportError> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or(ImportError::InvalidFormat("unexpected end of data"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, ImportError> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or(ImportError::InvalidFormat("unexpected end of data"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

