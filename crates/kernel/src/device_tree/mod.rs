pub mod bus_context;

pub use bus_context::DtBusContext;

use crate::{assert::static_assert_size, debug, info};
use core::{
    fmt::{Debug, Display},
    mem::size_of,
    ops::Range,
};
use hal::validated_ptr::ValidatedPtr;
use klib::{
    big_endian::BigEndian, parser::ConsumableBuffer, runtime_initialized::RuntimeInitializedData,
    util::UsizeExt,
};

const FDT_MAGIC: u32 = 0xd00dfeed;
const FDT_VERSION: u32 = 17;

pub static THE: RuntimeInitializedData<DeviceTree> = RuntimeInitializedData::new();

#[repr(C)]
#[derive(Debug, PartialEq, Eq)]
pub struct Header {
    magic: BigEndian<u32>,
    totalsize: BigEndian<u32>,
    off_dt_struct: BigEndian<u32>,
    off_dt_strings: BigEndian<u32>,
    off_mem_rsvmap: BigEndian<u32>,
    version: BigEndian<u32>,
    last_comp_version: BigEndian<u32>,
    boot_cpuid_phys: BigEndian<u32>,
    size_dt_strings: BigEndian<u32>,
    size_dt_struct: BigEndian<u32>,
}

static_assert_size!(Header, 40);

#[derive(Debug, PartialEq, Eq)]
pub struct DeviceTree {
    data: &'static [u8],
}

impl DeviceTree {
    fn new(device_tree_pointer: *const ()) -> Self {
        assert!(!device_tree_pointer.is_null());
        assert!(
            (device_tree_pointer as usize).is_multiple_of(size_of::<u32>()),
            "Device tree must be 4 byte aligned"
        );
        // Read just the header first to get totalsize.
        let dt_ptr = ValidatedPtr::<u8>::from_trusted(device_tree_pointer.cast());
        let header_slice = dt_ptr.as_static_slice(size_of::<Header>());
        let header: &Header = klib::util::ref_from_bytes(header_slice);
        assert_eq!(header.magic.get(), FDT_MAGIC);
        assert_eq!(
            header.version.get(),
            FDT_VERSION,
            "Device tree version mismatch"
        );
        let total_size = header.totalsize.get() as usize;
        Self {
            data: dt_ptr.as_static_slice(total_size),
        }
    }

    fn header(&self) -> &Header {
        klib::util::ref_from_bytes(self.data)
    }

    pub fn get_reserved_areas(&self) -> &[ReserveEntry] {
        let offset = self.header().off_mem_rsvmap.get() as usize;
        let remaining = &self.data[offset..];
        let entry_size = size_of::<ReserveEntry>();
        let max_entries = remaining.len() / entry_size;
        let mut len = 0;
        while len < max_entries {
            let entry: &ReserveEntry = klib::util::ref_from_bytes(&remaining[len * entry_size..]);
            if entry.address == 0 && entry.size == 0 {
                break;
            }
            len += 1;
        }
        klib::util::slice_from_bytes(remaining, 0, len)
    }

    pub fn root_node(&self) -> Node<'_> {
        let offset = self.header().off_dt_struct.get() as usize;
        let size = self.header().size_dt_struct.get() as usize;
        let data = &self.data[offset..offset + size];
        debug!("Structure Block Start: {:p}", data.as_ptr());
        let structure_block = ConsumableBuffer::new(data);
        let fake_node = Node::new("fake_node", self, structure_block);
        fake_node
            .find_node("")
            .expect("There must be a unnamed root-node")
    }

    fn get_string(&self, offset: usize) -> Option<&str> {
        let strings_offset = self.header().off_dt_strings.get() as usize;
        let strings_size = self.header().size_dt_strings.get() as usize;
        if offset >= strings_size {
            return None;
        }
        let strings_data = &self.data[strings_offset..strings_offset + strings_size];
        let mut consumable_buffer = ConsumableBuffer::new(&strings_data[offset..]);
        consumable_buffer.consume_str()
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct ReserveEntry {
    address: u64,
    size: u64,
}

impl Display for ReserveEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "RESERVED: {:#x} - {:#x} (size: {:#x})",
            self.address,
            self.address + self.size - 1,
            self.size
        )
    }
}

const FDT_BEGIN_NODE: u32 = 0x1;
const FDT_END_NODE: u32 = 0x2;
const FDT_PROP: u32 = 0x3;
const FDT_NOP: u32 = 0x4;
const FDT_END: u32 = 0x9;

#[derive(Debug, PartialEq, Eq)]
pub enum FdtToken<'a> {
    BeginNode(&'a str),
    EndNode,
    Prop(&'a str, ConsumableBuffer<'a>),
    Nop,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node<'a> {
    pub name: &'a str,
    pub address_cells: Option<u32>,
    pub size_cells: Option<u32>,
    pub parent_address_cells: Option<u32>,
    pub parent_size_cells: Option<u32>,
    device_tree: &'a DeviceTree,
    structure_block: ConsumableBuffer<'a>,
}

impl<'a> Node<'a> {
    fn new(
        name: &'a str,
        device_tree: &'a DeviceTree,
        structure_block: ConsumableBuffer<'a>,
    ) -> Self {
        let mut self_ = Self {
            name,
            device_tree,
            structure_block,
            address_cells: None,
            size_cells: None,
            parent_address_cells: None,
            parent_size_cells: None,
        };

        self_.address_cells = self_
            .get_property("#address-cells")
            .and_then(|mut b| b.consume_sized_type::<BigEndian<u32>>())
            .map(|be| be.get());
        self_.size_cells = self_
            .get_property("#size-cells")
            .and_then(|mut b| b.consume_sized_type::<BigEndian<u32>>())
            .map(|be| be.get());

        self_
    }

    pub fn find_node(&self, needle: &'a str) -> Option<Self> {
        let mut clone = self.clone();
        clone.find_node_recursive(needle)
    }

    pub fn find_node_by_phandle(&self, target_phandle: u32) -> Option<Self> {
        let mut clone = self.clone();
        clone.find_node_by_phandle_recursive(target_phandle)
    }

    fn find_node_by_phandle_recursive(&mut self, target_phandle: u32) -> Option<Self> {
        if let Some(mut phandle_buf) = self.get_property("phandle")
            && let Some(ph) = phandle_buf.consume_sized_type::<BigEndian<u32>>()
            && ph.get() == target_phandle
        {
            return Some(self.clone());
        }

        let mut parent_address_cell = self.address_cells;
        let mut parent_size_cell = self.size_cells;

        while let Some(token) = self.next() {
            match token {
                FdtToken::BeginNode(node_name) => {
                    let mut node =
                        Node::new(node_name, self.device_tree, self.structure_block.clone());
                    node.parent_address_cells = parent_address_cell;
                    node.parent_size_cells = parent_size_cell;
                    if let Some(target_node) = node.find_node_by_phandle_recursive(target_phandle) {
                        return Some(target_node);
                    }
                    self.structure_block = node.structure_block;
                }
                FdtToken::Prop(prop, mut data) => {
                    if prop == "#address-cells" {
                        parent_address_cell =
                            Some(data.consume_sized_type::<BigEndian<u32>>()?.get());
                    }
                    if prop == "#size-cells" {
                        parent_size_cell = Some(data.consume_sized_type::<BigEndian<u32>>()?.get());
                    }
                }
                FdtToken::Nop => {}
                FdtToken::EndNode | FdtToken::End => {
                    return None;
                }
            }
        }

        None
    }

    fn find_node_recursive(&mut self, needle: &'a str) -> Option<Self> {
        if self.name.split('@').next() == Some(needle) {
            return Some(self.clone());
        }

        let mut parent_address_cell = None;
        let mut parent_size_cell = None;

        while let Some(token) = self.next() {
            match token {
                FdtToken::BeginNode(node_name) => {
                    let mut node =
                        Node::new(node_name, self.device_tree, self.structure_block.clone());
                    node.parent_address_cells = parent_address_cell;
                    node.parent_size_cells = parent_size_cell;
                    if let Some(target_node) = node.find_node_recursive(needle) {
                        return Some(target_node);
                    }
                    self.structure_block = node.structure_block;
                }
                FdtToken::Prop(prop, mut data) => {
                    if prop == "#address-cells" {
                        parent_address_cell =
                            Some(data.consume_sized_type::<BigEndian<u32>>()?.get());
                    }
                    if prop == "#size-cells" {
                        parent_size_cell = Some(data.consume_sized_type::<BigEndian<u32>>()?.get());
                    }
                }
                FdtToken::Nop => {}
                FdtToken::EndNode | FdtToken::End => {
                    return None;
                }
            }
        }

        None
    }

    pub fn get_property(&self, name: &str) -> Option<ConsumableBuffer<'a>> {
        for token in self {
            match token {
                FdtToken::Prop(prop_name, data) => {
                    if prop_name == name {
                        return Some(data);
                    }
                }
                FdtToken::Nop => {}
                _ => break,
            }
        }
        None
    }

    pub fn children(&self) -> ChildrenIterator<'a> {
        ChildrenIterator {
            parent: self.clone(),
        }
    }

    pub fn parse_reg_property(&self) -> Option<Reg> {
        let mut reg_property = self.get_property("reg")?;
        self.parse_one_reg(&mut reg_property)
    }

    pub fn parse_all_reg_properties(&self) -> alloc::vec::Vec<Reg> {
        let mut regs = alloc::vec::Vec::new();
        let Some(mut reg_property) = self.get_property("reg") else {
            return regs;
        };
        while let Some(reg) = self.parse_one_reg(&mut reg_property) {
            regs.push(reg);
        }
        regs
    }

    fn parse_one_reg(&self, buf: &mut ConsumableBuffer<'a>) -> Option<Reg> {
        let address = match self.parent_address_cells? {
            1 => buf.consume_sized_type::<BigEndian<u32>>()?.get() as usize,
            2 => buf.consume_sized_type::<BigEndian<u64>>()?.get().as_usize(),
            _ => panic!("address cannot be larger than 64 bit"),
        };
        let size = match self.parent_size_cells? {
            1 => buf.consume_sized_type::<BigEndian<u32>>()?.get() as usize,
            2 => buf.consume_sized_type::<BigEndian<u64>>()?.get().as_usize(),
            _ => panic!("size cannot be larger than 64 bit"),
        };
        Some(Reg { address, size })
    }
}

pub struct Reg {
    pub address: usize,
    pub size: usize,
}

/// Returns true iff `[range.start, range.end)` is fully contained in a
/// single `reg` entry of a `/memory@*` node. Used to sanity-check
/// bootloader-provided physical ranges (e.g. initrd) before we hand
/// them to `ValidatedPtr::from_trusted` — a malformed bootloader
/// pointing at MMIO would otherwise be treated as RAM.
pub fn range_in_ram(range: Range<usize>) -> bool {
    if range.start >= range.end {
        return false;
    }
    for child in THE.root_node().children() {
        if child.name.split('@').next() != Some("memory") {
            continue;
        }
        for reg in child.parse_all_reg_properties() {
            let reg_end = reg.address.saturating_add(reg.size);
            if range.start >= reg.address && range.end <= reg_end {
                return true;
            }
        }
    }
    false
}

pub struct ChildrenIterator<'a> {
    parent: Node<'a>,
}

impl<'a> Iterator for ChildrenIterator<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Node<'a>> {
        loop {
            let token = self.parent.next()?;
            match token {
                FdtToken::BeginNode(name) => {
                    let child_sb = self.parent.structure_block.clone();
                    let mut child = Node::new(name, self.parent.device_tree, child_sb);
                    child.parent_address_cells = self.parent.address_cells;
                    child.parent_size_cells = self.parent.size_cells;

                    // Skip over child content in parent's stream
                    let mut depth = 0u32;
                    loop {
                        match self.parent.next()? {
                            FdtToken::BeginNode(_) => depth += 1,
                            FdtToken::EndNode => {
                                if depth == 0 {
                                    break;
                                }
                                depth -= 1;
                            }
                            FdtToken::End => return None,
                            _ => {}
                        }
                    }

                    return Some(child);
                }
                FdtToken::Prop(_, _) | FdtToken::Nop => continue,
                FdtToken::EndNode | FdtToken::End => return None,
            }
        }
    }
}

impl<'a> IntoIterator for &Node<'a> {
    type Item = FdtToken<'a>;

    type IntoIter = Node<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.clone()
    }
}

impl<'a> Iterator for Node<'a> {
    type Item = FdtToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.structure_block.empty() {
            return None;
        }

        let numeric_token_value = self
            .structure_block
            .consume_sized_type::<BigEndian<u32>>()?;
        let token = match numeric_token_value.get() {
            FDT_BEGIN_NODE => {
                let name = self.structure_block.consume_str()?;
                self.structure_block.consume_alignment(size_of::<u32>());
                FdtToken::BeginNode(name)
            }
            FDT_END_NODE => FdtToken::EndNode,
            FDT_PROP => {
                let len = self
                    .structure_block
                    .consume_sized_type::<BigEndian<u32>>()?
                    .get() as usize;
                let string_offset = self
                    .structure_block
                    .consume_sized_type::<BigEndian<u32>>()?
                    .get() as usize;
                let data = self.structure_block.consume_slice(len)?;
                self.structure_block.consume_alignment(size_of::<u32>());
                let string = self.device_tree.get_string(string_offset)?;
                FdtToken::Prop(string, ConsumableBuffer::new(data))
            }
            FDT_NOP => FdtToken::Nop,
            FDT_END => {
                assert!(self.structure_block.empty());
                FdtToken::End
            }
            _ => panic!("Unknown token: {:#x}", numeric_token_value.get()),
        };

        Some(token)
    }
}

pub fn get_devicetree_range() -> Range<*const u8> {
    let data = THE.data;
    data.as_ptr()..data.as_ptr().wrapping_add(data.len())
}

pub fn init(device_tree_pointer: *const ()) {
    info!("Initialize device tree at {device_tree_pointer:p}");
    let device_tree = DeviceTree::new(device_tree_pointer);
    let reserved = device_tree.get_reserved_areas();
    if !reserved.is_empty() {
        info!(
            "Device tree has {} reserved memory region(s)",
            reserved.len()
        );
    }
    THE.initialize(device_tree);
}

#[cfg(test)]
mod tests {
    use super::Node;
    use crate::{
        device_tree::{DeviceTree, Header},
        info,
    };
    use abi::include_bytes_align_as;
    use klib::big_endian::BigEndian;

    const DTB: &[u8] = include_bytes_align_as!(Header, "../test/test_data/dtb");

    // Static DeviceTree to avoid Box::leak per test (miri leak check).
    static TEST_DT: klib::runtime_initialized::RuntimeInitializedData<DeviceTree> =
        klib::runtime_initialized::RuntimeInitializedData::new();

    fn ensure_dt_initialized() {
        if !TEST_DT.is_initialized() {
            let device_tree = DeviceTree::new(DTB.as_ptr().cast::<()>());
            assert!(device_tree.header().totalsize.get() as usize <= DTB.len());
            TEST_DT.initialize(device_tree);
        }
    }

    fn get_root_node() -> Node<'static> {
        ensure_dt_initialized();
        TEST_DT.root_node()
    }

    #[test_case]
    fn basic_values() {
        let root_node = get_root_node();

        assert_eq!(
            root_node
                .get_property("compatible")
                .and_then(|mut b| b.consume_str()),
            Some("riscv-virtio")
        );
        assert_eq!(
            root_node
                .get_property("model")
                .and_then(|mut b| b.consume_str()),
            Some("riscv-virtio,qemu")
        );
        assert_eq!(root_node.get_property("foobar"), None);
    }

    #[test_case]
    fn inexistent_node() {
        let root_node = get_root_node();
        assert!(root_node.find_node("foobar").is_none());
    }

    #[test_case]
    fn single_depth_node() {
        let root_node = get_root_node();

        let chosen = root_node
            .find_node("chosen")
            .expect("chosen node must exist");

        assert!(chosen.address_cells.is_none());
        assert!(chosen.size_cells.is_none());

        assert_eq!(chosen.parent_address_cells, root_node.address_cells);
        assert_eq!(chosen.parent_size_cells, root_node.size_cells);

        assert_eq!(
            chosen
                .get_property("rng-seed")
                .and_then(|mut b| b.consume_sized_type::<BigEndian<u32>>())
                .map(|big_endian| big_endian.get()),
            Some(0x6164a749)
        );
        assert_eq!(
            chosen
                .get_property("stdout-path")
                .and_then(|mut b| b.consume_str()),
            Some("/soc/serial@10000000")
        );
    }

    #[test_case]
    fn multiple_depth_node() {
        let root_node = get_root_node();

        let cpu0 = root_node.find_node("cpu").expect("cpu node must exist");

        assert_eq!(cpu0.name, "cpu@0");
        assert_eq!(cpu0.parent_address_cells, Some(1));
        assert_eq!(cpu0.parent_size_cells, Some(0));

        assert_eq!(
            cpu0.get_property("riscv,cboz-block-size")
                .and_then(|mut b| b.consume_sized_type::<BigEndian<u32>>())
                .map(|big_endian| big_endian.get()),
            Some(0x40)
        );

        assert!(
            cpu0.get_property("#interrupt-cells").is_none(),
            "Must not access nested nodes."
        );

        let interrupt_controller_cpu0 = cpu0
            .find_node("interrupt-controller")
            .expect("interrupt controller must be accessible.");
        let interrupt_controller_root_node = root_node
            .find_node("interrupt-controller")
            .expect("interrupt controller must be accessible.");

        assert_eq!(
            interrupt_controller_cpu0, interrupt_controller_root_node,
            "Node must be the same independent where we got it from."
        );
    }

    #[test_case]
    fn cells() {
        let root_node = get_root_node();

        assert!(root_node.parent_address_cells.is_none());
        assert!(root_node.parent_size_cells.is_none());

        assert_eq!(root_node.address_cells, Some(2));
        assert_eq!(root_node.size_cells, Some(2));

        assert_cells("poweroff", Some(2), Some(2), None, None);
        assert_cells("platform-bus", Some(2), Some(2), Some(1), Some(1));
        assert_cells("memory", Some(2), Some(2), None, None);
        assert_cells("cpu", Some(1), Some(0), None, None);
        assert_cells("interrupt-controller", None, None, None, None);
    }

    fn assert_cells(
        node_name: &str,
        parent_address_cells: Option<u32>,
        parent_size_cells: Option<u32>,
        address_cells: Option<u32>,
        size_cells: Option<u32>,
    ) {
        let root_node = get_root_node();

        let node = root_node.find_node(node_name).expect("node must exist");

        assert_eq!(node.parent_address_cells, parent_address_cells);
        assert_eq!(node.parent_size_cells, parent_size_cells);

        assert_eq!(node.address_cells, address_cells);
        assert_eq!(node.size_cells, size_cells);
    }
}
