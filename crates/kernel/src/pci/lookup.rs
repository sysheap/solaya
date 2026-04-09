use alloc::string::String;

static PCI_IDS: &str = include_str!("pci.ids");

pub fn lookup(vendor_id: u16, device_id: u16) -> Option<String> {
    let mut lines = PCI_IDS.lines();
    let mut vendor = "";
    // Look for vendor
    for line in lines.by_ref() {
        if line.starts_with(format!("{vendor_id:04x}").as_str()) {
            vendor = &line[6..];
            break;
        }
    }
    // Look for device
    for line in lines.by_ref() {
        // Break if we are at the next vendor
        if !line.starts_with('\t') {
            return None;
        }

        if line.starts_with(format!("\t{device_id:04x}").as_str()) {
            let device = &line[7..];
            return Some(format!("{vendor} - {device}"));
        }
    }
    None
}
