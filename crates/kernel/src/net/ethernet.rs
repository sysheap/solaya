use core::fmt::Display;

use crate::{
    assert::static_assert_size,
    debug,
    klibc::{
        big_endian::BigEndian,
        util::{BufferExtension, ByteInterpretable},
    },
};

use super::{current_mac_address, mac::MacAddress};

const BROADCAST_MAC: MacAddress = MacAddress::new([0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);

#[derive(Debug)]
#[repr(C)]
pub struct EthernetHeader {
    destination_mac: MacAddress,
    source_mac: MacAddress,
    pub ether_type: BigEndian<u16>,
    // data: [u8],
    // chksum: u32,
}

static_assert_size!(EthernetHeader, 14);

impl ByteInterpretable for EthernetHeader {}

#[derive(Debug)]
pub enum ParseError {
    PacketTooSmall,
    UnknownEtherType,
    UnknownDestinationMac,
}

const ETHERTYPE_ARP: u16 = 0x0806;
#[allow(non_upper_case_globals)]
const ETHERTYPE_IPV4: u16 = 0x0800;

#[derive(Debug)]
pub enum EtherTypes {
    Arp,
    IPv4,
}

impl TryFrom<BigEndian<u16>> for EtherTypes {
    type Error = ParseError;

    fn try_from(value: BigEndian<u16>) -> Result<Self, Self::Error> {
        match value.get() {
            ETHERTYPE_ARP => Ok(EtherTypes::Arp),
            ETHERTYPE_IPV4 => Ok(EtherTypes::IPv4),
            _ => Err(ParseError::UnknownEtherType),
        }
    }
}

impl From<EtherTypes> for BigEndian<u16> {
    fn from(value: EtherTypes) -> Self {
        match value {
            EtherTypes::Arp => BigEndian::from_little_endian(ETHERTYPE_ARP),
            EtherTypes::IPv4 => BigEndian::from_little_endian(ETHERTYPE_IPV4),
        }
    }
}

impl EthernetHeader {
    // const CHECKSUM_LENGTH: usize = core::mem::size_of::<u32>();
    const MIN_LENGTH: usize = core::mem::size_of::<EthernetHeader>(); // 4 byte checksum at the end

    pub fn new(
        destination_mac: MacAddress,
        source_mac: MacAddress,
        ether_type: EtherTypes,
    ) -> Self {
        Self {
            destination_mac,
            source_mac,
            ether_type: ether_type.into(),
        }
    }

    pub fn try_parse(data: &[u8]) -> Result<(&Self, &[u8]), ParseError> {
        if data.len() < Self::MIN_LENGTH {
            return Err(ParseError::PacketTooSmall);
        }
        let (header, rest) = data.split_as::<EthernetHeader>();

        if !header.is_valid_ether_type() {
            return Err(ParseError::UnknownEtherType);
        }

        if header.destination_mac != current_mac_address()
            && header.destination_mac != BROADCAST_MAC
        {
            debug!(
                "Unknown destination mac: {}; NIC mac: {}",
                header.destination_mac,
                current_mac_address()
            );
            return Err(ParseError::UnknownDestinationMac);
        }

        Ok((header, rest))
    }

    fn is_valid_ether_type(&self) -> bool {
        EtherTypes::try_from(self.ether_type).is_ok()
    }

    pub fn ether_type(&self) -> EtherTypes {
        EtherTypes::try_from(self.ether_type).expect("Must be already parsed.")
    }

    pub fn source_mac(&self) -> MacAddress {
        self.source_mac
    }
}

impl Display for EthernetHeader {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Ethernet packet: destination_mac: {}, source_mac: {}, ether_type: {:?}",
            self.destination_mac,
            self.source_mac,
            self.ether_type(),
        )
    }
}
