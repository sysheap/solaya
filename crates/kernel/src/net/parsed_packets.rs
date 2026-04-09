use super::{arp::ArpPacket, ethernet::EthernetHeader};

pub enum ParsedPacket {
    Arp(EthernetHeader, ArpPacket),
}
