use alloc::vec::Vec;
use core::net::Ipv4Addr;

use crate::{
    assert::static_assert_size,
    debug,
    klibc::{big_endian::BigEndian, util::ByteInterpretable},
    net::ethernet::EthernetHeader,
};

use super::{DRIVER_HEADER_RESERVE, ipv4::IpV4Header, mac::MacAddress};

pub const FLAG_FIN: u16 = headers::socket::TH_FIN as u16;
pub const FLAG_SYN: u16 = headers::socket::TH_SYN as u16;
pub const FLAG_RST: u16 = headers::socket::TH_RST as u16;
pub const FLAG_ACK: u16 = headers::socket::TH_ACK as u16;

const OPT_END: u8 = 0;
const OPT_NOP: u8 = 1;
const OPT_MSS: u8 = 2;
const OPT_WINDOW_SCALE: u8 = 3;

#[derive(Debug, Clone, Copy, Default)]
pub struct TcpOptions {
    pub mss: Option<u16>,
    pub window_scale: Option<u8>,
}

impl TcpOptions {
    fn parse(data: &[u8]) -> Self {
        let mut opts = Self::default();
        let mut i = 0;
        while i < data.len() {
            match data[i] {
                OPT_END => break,
                OPT_NOP => i += 1,
                OPT_MSS if i + 3 < data.len() && data[i + 1] == 4 => {
                    opts.mss = Some(u16::from_be_bytes([data[i + 2], data[i + 3]]));
                    i += 4;
                }
                OPT_WINDOW_SCALE if i + 2 < data.len() && data[i + 1] == 3 => {
                    opts.window_scale = Some(data[i + 2]);
                    i += 3;
                }
                _ => {
                    if i + 1 >= data.len() {
                        break;
                    }
                    let len = data[i + 1] as usize;
                    if len < 2 || i + len > data.len() {
                        break;
                    }
                    i += len;
                }
            }
        }
        opts
    }
}

pub fn build_syn_options(mss: u16, window_scale: u8) -> [u8; 8] {
    let mss_bytes = mss.to_be_bytes();
    [
        OPT_MSS,
        4,
        mss_bytes[0],
        mss_bytes[1],
        OPT_NOP,
        OPT_WINDOW_SCALE,
        3,
        window_scale,
    ]
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct TcpHeader {
    source_port: BigEndian<u16>,
    destination_port: BigEndian<u16>,
    sequence_number: BigEndian<u32>,
    acknowledgment_number: BigEndian<u32>,
    data_offset_and_flags: BigEndian<u16>,
    window_size: BigEndian<u16>,
    checksum: BigEndian<u16>,
    urgent_pointer: BigEndian<u16>,
}

static_assert_size!(TcpHeader, 20);

impl ByteInterpretable for TcpHeader {}

impl TcpHeader {
    const HEADER_SIZE: usize = core::mem::size_of::<Self>();
    const TCP_PROTOCOL: u8 = headers::socket::IPPROTO_TCP as u8;

    pub fn source_port(&self) -> u16 {
        self.source_port.get()
    }

    pub fn destination_port(&self) -> u16 {
        self.destination_port.get()
    }

    pub fn sequence_number(&self) -> u32 {
        self.sequence_number.get()
    }

    pub fn acknowledgment_number(&self) -> u32 {
        self.acknowledgment_number.get()
    }

    pub fn flags(&self) -> u16 {
        self.data_offset_and_flags.get() & 0x1FF
    }

    pub fn window_size(&self) -> u16 {
        self.window_size.get()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_tcp_packet(
        destination_ip: Ipv4Addr,
        destination_mac: MacAddress,
        source_port: u16,
        destination_port: u16,
        seq: u32,
        ack: u32,
        flags: u16,
        window: u16,
        data: &[u8],
        options: &[u8],
    ) -> Vec<u8> {
        assert!(
            options.len().is_multiple_of(4),
            "TCP options must be 4-byte aligned"
        );
        let data_offset = 5 + (options.len() / 4) as u16;
        let data_offset_and_flags = (data_offset << 12) | (flags & 0x1FF);
        let mut tcp_header = Self {
            source_port: BigEndian::from_little_endian(source_port),
            destination_port: BigEndian::from_little_endian(destination_port),
            sequence_number: BigEndian::from_little_endian(seq),
            acknowledgment_number: BigEndian::from_little_endian(ack),
            data_offset_and_flags: BigEndian::from_little_endian(data_offset_and_flags),
            window_size: BigEndian::from_little_endian(window),
            checksum: BigEndian::from_little_endian(0),
            urgent_pointer: BigEndian::from_little_endian(0),
        };

        let mut ip_header = IpV4Header::new(
            destination_ip,
            Self::TCP_PROTOCOL,
            Self::HEADER_SIZE + options.len() + data.len(),
        );

        tcp_header.checksum = BigEndian::from_little_endian(Self::compute_checksum(
            options,
            data,
            &tcp_header,
            &ip_header,
        ));

        ip_header.header_checksum = BigEndian::from_little_endian(ip_header.calculate_checksum());

        let ethernet_header = EthernetHeader::new(
            destination_mac,
            super::current_mac_address(),
            crate::net::ethernet::EtherTypes::IPv4,
        );

        let frame_len = ethernet_header.as_slice().len()
            + ip_header.as_slice().len()
            + tcp_header.as_slice().len()
            + options.len()
            + data.len();
        let mut packet = Vec::with_capacity(DRIVER_HEADER_RESERVE + frame_len);
        packet.extend_from_slice(&[0u8; DRIVER_HEADER_RESERVE]);
        packet.extend_from_slice(ethernet_header.as_slice());
        packet.extend_from_slice(ip_header.as_slice());
        packet.extend_from_slice(tcp_header.as_slice());
        packet.extend_from_slice(options);
        packet.extend_from_slice(data);

        debug!("Sending TCP packet with size {}", frame_len);

        packet
    }

    pub fn process<'a>(
        data: &'a [u8],
        ip_header: &IpV4Header,
    ) -> Result<(TcpHeader, TcpOptions, &'a [u8]), TcpParseError> {
        if data.len() < Self::HEADER_SIZE {
            return Err(TcpParseError::PacketTooSmall);
        }

        let tcp_header: TcpHeader = sys::klibc::util::read_from_bytes(data);

        let data_offset = usize::from(tcp_header.data_offset_and_flags.get() >> 12);
        if data_offset < 5 {
            return Err(TcpParseError::InvalidDataOffset);
        }

        let header_bytes = data_offset * 4;
        if data.len() < header_bytes {
            return Err(TcpParseError::PacketTooSmall);
        }

        let options = if header_bytes > Self::HEADER_SIZE {
            TcpOptions::parse(&data[Self::HEADER_SIZE..header_bytes])
        } else {
            TcpOptions::default()
        };

        let payload = &data[header_bytes..];

        let total_tcp_len =
            usize::from(ip_header.total_packet_length.get()) - IpV4Header::HEADER_SIZE;
        let payload_len = total_tcp_len - header_bytes;
        let payload = &payload[..payload_len];

        let computed = Self::compute_checksum_raw(&data[..total_tcp_len], ip_header, total_tcp_len);
        if computed != 0 {
            return Err(TcpParseError::InvalidChecksum);
        }

        Ok((tcp_header, options, payload))
    }

    fn pseudo_header(ip_header: &IpV4Header, tcp_length: usize) -> [u8; 12] {
        let mut pseudo_header = [0u8; 12];
        pseudo_header[0..4].copy_from_slice(&ip_header.source_ip.octets());
        pseudo_header[4..8].copy_from_slice(&ip_header.destination_ip.octets());
        pseudo_header[9] = Self::TCP_PROTOCOL;
        pseudo_header[10..12].copy_from_slice(
            &u16::try_from(tcp_length)
                .expect("TCP length must fit in u16")
                .to_be_bytes(),
        );
        pseudo_header
    }

    fn compute_checksum(
        options: &[u8],
        data: &[u8],
        tcp_header: &TcpHeader,
        ip_header: &IpV4Header,
    ) -> u16 {
        let tcp_length = Self::HEADER_SIZE + options.len() + data.len();
        let pseudo = Self::pseudo_header(ip_header, tcp_length);
        super::checksum::ones_complement_checksum(&[&pseudo, tcp_header.as_slice(), options, data])
    }

    fn compute_checksum_raw(tcp_bytes: &[u8], ip_header: &IpV4Header, tcp_length: usize) -> u16 {
        let pseudo = Self::pseudo_header(ip_header, tcp_length);
        super::checksum::ones_complement_checksum(&[&pseudo, tcp_bytes])
    }
}

#[derive(Debug)]
pub enum TcpParseError {
    PacketTooSmall,
    InvalidDataOffset,
    InvalidChecksum,
}

#[cfg(test)]
mod tests {
    use crate::{klibc::big_endian::BigEndian, net::ipv4::IpV4Header};
    use core::net::Ipv4Addr;

    use super::TcpHeader;

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
            source_ip: Ipv4Addr::new(10, 0, 2, 15),
            destination_ip: Ipv4Addr::new(10, 0, 2, 2),
        };

        let tcp_header = TcpHeader {
            source_port: BigEndian::from_little_endian(1234),
            destination_port: BigEndian::from_little_endian(80),
            sequence_number: BigEndian::from_little_endian(100),
            acknowledgment_number: BigEndian::from_little_endian(0),
            data_offset_and_flags: BigEndian::from_little_endian((5u16 << 12) | 0x002),
            window_size: BigEndian::from_little_endian(8192),
            checksum: BigEndian::from_little_endian(0),
            urgent_pointer: BigEndian::from_little_endian(0),
        };

        let data = b"";
        let checksum = TcpHeader::compute_checksum(&[], data, &tcp_header, &ip_header);

        let tcp_header_with_checksum = TcpHeader {
            checksum: BigEndian::from_little_endian(checksum),
            ..tcp_header
        };

        let verify = TcpHeader::compute_checksum(&[], data, &tcp_header_with_checksum, &ip_header);
        assert_eq!(verify, 0);
    }

    #[test_case]
    fn checksum_with_data() {
        let ip_header = IpV4Header {
            version_and_ihl: BigEndian::from_little_endian(0),
            tos: BigEndian::from_little_endian(0),
            total_packet_length: BigEndian::from_little_endian(0),
            identification: BigEndian::from_little_endian(0),
            flags_and_offset: BigEndian::from_little_endian(0),
            ttl: BigEndian::from_little_endian(0),
            upper_protocol: BigEndian::from_little_endian(0),
            header_checksum: BigEndian::from_little_endian(0),
            source_ip: Ipv4Addr::new(192, 168, 1, 100),
            destination_ip: Ipv4Addr::new(192, 168, 1, 1),
        };

        let tcp_header = TcpHeader {
            source_port: BigEndian::from_little_endian(5000),
            destination_port: BigEndian::from_little_endian(80),
            sequence_number: BigEndian::from_little_endian(1000),
            acknowledgment_number: BigEndian::from_little_endian(2000),
            data_offset_and_flags: BigEndian::from_little_endian((5u16 << 12) | 0x018),
            window_size: BigEndian::from_little_endian(8192),
            checksum: BigEndian::from_little_endian(0),
            urgent_pointer: BigEndian::from_little_endian(0),
        };

        let data = b"Hello TCP!";
        let checksum = TcpHeader::compute_checksum(&[], data, &tcp_header, &ip_header);

        let tcp_header_with_checksum = TcpHeader {
            checksum: BigEndian::from_little_endian(checksum),
            ..tcp_header
        };

        let verify = TcpHeader::compute_checksum(&[], data, &tcp_header_with_checksum, &ip_header);
        assert_eq!(verify, 0);
    }

    #[test_case]
    fn flags_extraction() {
        let header = TcpHeader {
            source_port: BigEndian::from_little_endian(0),
            destination_port: BigEndian::from_little_endian(0),
            sequence_number: BigEndian::from_little_endian(0),
            acknowledgment_number: BigEndian::from_little_endian(0),
            data_offset_and_flags: BigEndian::from_little_endian((5u16 << 12) | 0x012),
            window_size: BigEndian::from_little_endian(0),
            checksum: BigEndian::from_little_endian(0),
            urgent_pointer: BigEndian::from_little_endian(0),
        };

        assert_eq!(header.flags(), 0x012);
        assert!(header.flags() & super::FLAG_SYN != 0);
        assert!(header.flags() & super::FLAG_ACK != 0);
        assert!(header.flags() & super::FLAG_FIN == 0);
    }
}
