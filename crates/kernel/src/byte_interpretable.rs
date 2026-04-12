/// Kernel-local extension trait mirroring `klib::util::ByteInterpretable`.
///
/// We need a locally-defined trait so the orphan rule allows implementing it
/// for foreign types (e.g. `headers::fs::stat`, `sockaddr_in`).
pub trait ByteInterpretable {
    fn as_slice(&self) -> &[u8] {
        klib::util::as_byte_slice(self)
    }
}
