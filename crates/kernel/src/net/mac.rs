//! Re-exports `driver_api::MacAddress`.
//!
//! The MAC-address newtype lives in `driver-api` because the `NetDevice`
//! trait names it. The kernel keeps this module as the historical import
//! path so existing `net::mac::MacAddress` uses continue to work.

pub use driver_api::MacAddress;
