//! `DriverFactory` for the StarFive JH7110 DWMAC ethernet controller.
//!
//! Probes the device tree through `DtBusContextExt` (compatible string
//! `starfive,jh7110-eqos-5.20`); on a match, runs the JH7110 clock/reset/
//! syscon bring-up and spins up the core DWMAC logic.

use alloc::{boxed::Box, sync::Arc};

use driver_api::{
    BusContext, DriverFactory, DriverInstance, DtBusContextExt, IrqId, MacAddress, ProbeError,
};
use klib::{big_endian::BigEndian, parser::FromU8Buffer};

use super::{DwmacDevice, DwmacHandle, jh7110};

const JH7110_COMPATIBLE: &str = "starfive,jh7110-eqos-5.20";

pub struct DwmacDtFactory;

impl DriverFactory for DwmacDtFactory {
    fn name(&self) -> &'static str {
        "dwmac-jh7110"
    }

    fn probe(&self, bus: &dyn BusContext) -> bool {
        bus.as_dt()
            .and_then(|dt| dt.compatible())
            .is_some_and(|c| c == JH7110_COMPATIBLE)
    }

    fn attach(&self, bus: &dyn BusContext) -> Result<DriverInstance, ProbeError> {
        let dt = bus
            .as_dt()
            .ok_or(ProbeError::InitializationFailed("dwmac: non-DT bus"))?;

        let reg_base = dt.reg_base();
        let mac = parse_mac(dt)?;
        let irq = dt
            .first_interrupt()
            .ok_or(ProbeError::InitializationFailed("dwmac: missing IRQ"))?;
        let clock_ids = parse_phandle_ids(dt, "clocks");
        let reset_ids = parse_phandle_ids(dt, "resets");

        // Only GMAC1 (0x1604_0000) is wired up today; other instances can
        // be added when we have hardware to exercise them.
        let gmac_index = match reg_base {
            0x1604_0000 => 1u8,
            _ => {
                return Err(ProbeError::InitializationFailed(
                    "dwmac: unknown register base",
                ));
            }
        };

        jh7110::init_gmac(gmac_index, &clock_ids, &reset_ids);

        let device = DwmacDevice::new(reg_base, mac, gmac_index as u32).ok_or(
            ProbeError::InitializationFailed("dwmac: hardware init failed"),
        )?;
        let handle = Arc::new(DwmacHandle::new(device));
        let irq_handler: Arc<dyn driver_api::IrqHandler> = handle.clone();
        let registration = bus
            .register_irq(IrqId(irq), irq_handler)
            .map_err(|_| ProbeError::InitializationFailed("dwmac: irq registration failed"))?;
        handle.set_irq_registration(registration);
        Ok(DriverInstance::Net(handle))
    }
}

fn parse_mac(dt: &dyn DtBusContextExt) -> Result<MacAddress, ProbeError> {
    let bytes = dt
        .property_bytes("local-mac-address")
        .ok_or(ProbeError::InitializationFailed(
            "dwmac: no local-mac-address",
        ))?;
    if bytes.len() < 6 {
        return Err(ProbeError::InitializationFailed(
            "dwmac: local-mac-address too short",
        ));
    }
    Ok(MacAddress::new([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5],
    ]))
}

/// Parse a DT property shaped as `(phandle: u32, id: u32)` pairs (the
/// encoding used for `clocks` and `resets`), returning just the IDs.
fn parse_phandle_ids(dt: &dyn DtBusContextExt, prop_name: &str) -> alloc::vec::Vec<u32> {
    let mut ids = alloc::vec::Vec::new();
    let Some(bytes) = dt.property_bytes(prop_name) else {
        return ids;
    };
    // Each entry is two big-endian u32s; skip the phandle, keep the id.
    let mut offset = 0;
    while offset + 8 <= bytes.len() {
        let id_bytes = &bytes[offset + 4..offset + 8];
        let id = BigEndian::<u32>::from_u8_buffer(id_bytes).get();
        ids.push(id);
        offset += 8;
    }
    ids
}

/// Registers the JH7110 DWMAC factory with `catalog`. Called from
/// `register_builtin`.
pub fn register(catalog: &mut driver_api::DriverCatalog) {
    catalog.register(Box::new(DwmacDtFactory));
}
