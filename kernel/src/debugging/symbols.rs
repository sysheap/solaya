#![allow(unsafe_code)]
use crate::{
    debug, info, klibc::runtime_initialized::RuntimeInitializedData,
    memory::linker_information::LinkerInformation,
};

pub static THE: RuntimeInitializedData<&'static str> = RuntimeInitializedData::new();

pub fn init() {
    let symbols_start = LinkerInformation::__start_symbols();
    // SAFETY: The symbols section is null-terminated by the build process
    // (objcopy --update-section appends a NUL byte).
    let cstr = unsafe { core::ffi::CStr::from_ptr(symbols_start.as_ptr()) };
    let str = cstr.to_str().expect("Symbols must be UTF-8");
    info!("Initialized symbols ({} bytes)", str.len());
    THE.initialize(str);
}

pub fn symbols_end() -> usize {
    let size = symbols_size();
    let symbols_start = LinkerInformation::__start_symbols();
    (symbols_start + size).as_usize()
}

#[cfg(not(miri))]
pub fn symbols_size() -> usize {
    // Make sure we include the nullbyte
    THE.len() + 1
}

#[cfg(miri)]
pub fn symbols_size() -> usize {
    1
}

pub struct AddressAndSymbol {
    pub address: usize,
    pub symbol: &'static str,
    pub file: Option<&'static str>,
}

pub fn get_symbol(target_address: usize) -> Option<AddressAndSymbol> {
    debug!("Get symbol for {target_address:#x}");
    let mut previous = None;
    for line in THE.lines() {
        let mut parts = line.split('\t');
        let first = parts
            .next()
            .expect("There should be a first part of the line");
        let file = parts.next();

        let mut parts = first.split_whitespace();
        let address_str = parts.next().expect("Address should be the first element");
        let address: usize =
            usize::from_str_radix(address_str, 16).expect("Symbols address must be parsable");
        // Ignore type of symbols
        parts.next();
        let symbol = parts.next().expect("The symbol name must exist");
        debug!("Looking at {address:#x} {symbol}");
        if address > target_address {
            break;
        }
        previous = Some(AddressAndSymbol {
            address,
            symbol,
            file,
        })
    }
    previous
}
