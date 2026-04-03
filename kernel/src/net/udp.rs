use alloc::vec::Vec;
use core::net::Ipv4Addr;

use crate::{
    assert::static_assert_size,
    debug,
    klibc::{
        big_endian::BigEndian,
        util::{BufferExtension, ByteInterpretable},
    },
    net::ethernet::EthernetHeader,
};

use super::{ipv4::IpV4Header, mac::MacAddress, new_packet_buffer};

#[derive(Debug)]
#[repr(C)]
pub struct UdpHeader {
    source_port: BigEndian<u16>,
    destination_port: BigEndian<u16>,
    length: BigEndian<u16>,
    checksum: BigEndian<u16>,
}

static_assert_size!(UdpHeader, 8);

impl ByteInterpretable for UdpHeader {}

#[derive(Debug)]
pub enum UdpParseError {
    PacketTooSmall,
}

impl UdpHeader {
    const UDP_HEADER_SIZE: usize = core::mem::size_of::<Self>();
    const UDP_PROTOCOL_TYPE: u8 = 17;

    pub fn destination_port(&self) -> u16 {
        self.destination_port.get()
    }
    pub fn source_port(&self) -> u16 {
        self.source_port.get()
    }

    pub fn create_udp_packet(
        destination_ip: Ipv4Addr,
        destination_port: u16,
        destination_mac: MacAddress,
        source_port: u16,
        data: &[u8],
    ) -> Vec<u8> {
        let mut udp_header = Self {
            source_port: BigEndian::from_little_endian(source_port),
            destination_port: BigEndian::from_little_endian(destination_port),
            length: BigEndian::from_little_endian(
                u16::try_from(Self::UDP_HEADER_SIZE + data.len())
                    .expect("Size must not exceed u16"),
            ),
            checksum: BigEndian::from_little_endian(0),
        };

        let mut ip_header = IpV4Header::new(
            destination_ip,
            Self::UDP_PROTOCOL_TYPE,
            Self::UDP_HEADER_SIZE + data.len(),
        );

        udp_header.checksum =
            BigEndian::from_little_endian(Self::compute_checksum(data, &udp_header, &ip_header));

        ip_header.header_checksum = BigEndian::from_little_endian(ip_header.calculate_checksum());

        let ethernet_header = EthernetHeader::new(
            destination_mac,
            super::current_mac_address(),
            crate::net::ethernet::EtherTypes::IPv4,
        );

        let frame_len = ethernet_header.as_slice().len()
            + ip_header.as_slice().len()
            + udp_header.as_slice().len()
            + data.len();
        let mut packet = new_packet_buffer(frame_len);
        packet.extend_from_slice(ethernet_header.as_slice());
        packet.extend_from_slice(ip_header.as_slice());
        packet.extend_from_slice(udp_header.as_slice());
        packet.extend_from_slice(data);

        debug!("Sending UDP packet with size {}", frame_len);

        packet
    }

    pub fn process<'a>(
        data: &'a [u8],
        ip_header: &IpV4Header,
    ) -> Result<(&'a UdpHeader, &'a [u8]), UdpParseError> {
        if data.len() < Self::UDP_HEADER_SIZE {
            return Err(UdpParseError::PacketTooSmall);
        }

        let (udp_header, rest) = data.split_as::<UdpHeader>();

        debug!(
            "Received udp packet; Header tells {:#x} length and we got {:#x} rest of data",
            udp_header.length.get(),
            rest.len()
        );
        assert!(
            rest.len() + Self::UDP_HEADER_SIZE >= udp_header.length.get() as usize,
            "The length field must have a valid value."
        );

        // Truncate data field
        let data_length = udp_header.length.get() as usize - Self::UDP_HEADER_SIZE;
        let rest = &rest[..data_length];

        if udp_header.checksum.get() != 0 {
            debug!("Got checksum: {:#x}", udp_header.checksum.get());
            let computed_checksum = Self::compute_checksum(rest, udp_header, ip_header);
            assert_eq!(computed_checksum, 0, "must be zero for a valid packet.");
        }

        Ok((udp_header, rest))
    }

    fn compute_checksum(data: &[u8], udp_header: &UdpHeader, ip_header: &IpV4Header) -> u16 {
        assert_eq!(
            data.len(),
            udp_header.length.get() as usize - UdpHeader::UDP_HEADER_SIZE
        );

        let mut pseudo_header = [0u8; 12];
        pseudo_header[0..4].copy_from_slice(&ip_header.source_ip.octets());
        pseudo_header[4..8].copy_from_slice(&ip_header.destination_ip.octets());
        pseudo_header[9] = Self::UDP_PROTOCOL_TYPE;
        pseudo_header[10..12].copy_from_slice(&udp_header.length.get().to_be_bytes());

        super::checksum::ones_complement_checksum(&[&pseudo_header, udp_header.as_slice(), data])
    }
}

#[cfg(test)]
mod tests {
    use crate::{klibc::big_endian::BigEndian, net::ipv4::IpV4Header};
    use core::net::Ipv4Addr;

    use super::UdpHeader;

    #[test_case]
    fn checksum_calculation() {
        let ip_header = IpV4Header {
            version_and_ihl: BigEndian::from_little_endian(0),
            tos: BigEndian::from_little_endian(0),
            total_packet_length: BigEndian::from_little_endian(0),
            identification: BigEndian::from_little_endian(0),
            flags_and_offset: BigEndian::from_little_endian(0),
            ttl: BigEndian::from_little_endian(0),
            upper_protocol: BigEndian::from_little_endian(0),
            header_checksum: BigEndian::from_little_endian(0),
            source_ip: Ipv4Addr::new(10, 0, 2, 2),
            destination_ip: Ipv4Addr::new(10, 0, 2, 15),
        };

        let udp_header = UdpHeader {
            source_port: BigEndian::from_little_endian(33015),
            destination_port: BigEndian::from_little_endian(1234),
            length: BigEndian::from_little_endian(21),
            checksum: BigEndian::from_little_endian(0x05fb),
        };

        let data = "Hello World!\n";

        let calculated_checksum =
            UdpHeader::compute_checksum(data.as_bytes(), &udp_header, &ip_header);

        assert_eq!(calculated_checksum, 0);
    }
}
