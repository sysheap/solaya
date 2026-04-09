/// RFC 1071 one's complement checksum over multiple byte slices.
/// Correctly handles cross-slice odd bytes.
pub fn ones_complement_checksum(slices: &[&[u8]]) -> u16 {
    let mut sum = 0u32;
    let mut leftover: Option<u8> = None;

    for slice in slices {
        let mut i = 0;

        if let Some(high) = leftover.take() {
            if !slice.is_empty() {
                sum += ((high as u16) << 8 | slice[0] as u16) as u32;
                i = 1;
            } else {
                leftover = Some(high);
                continue;
            }
        }

        while i + 1 < slice.len() {
            sum += ((slice[i] as u16) << 8 | slice[i + 1] as u16) as u32;
            i += 2;
        }

        if i < slice.len() {
            leftover = Some(slice[i]);
        }
    }

    if let Some(high) = leftover {
        sum += (high as u32) << 8;
    }

    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }

    !u16::try_from(sum).expect("carry fold ensures 16-bit result")
}
