use alloc::{collections::BTreeMap, string::ToString, vec::Vec};
use common::errors::LoaderError;
use headers::syscall_types::{AT_NULL, AT_PAGESZ, AT_PHDR, AT_PHENT, AT_PHNUM, AT_RANDOM};

use crate::klibc::{util::align_up, writable_buffer::WritableBuffer};

use crate::{
    debug,
    klibc::{
        elf::{ElfFile, ProgramHeaderType},
        util::{InBytes, UsizeExt, minimum_amount_of_pages},
    },
    memory::{
        PAGE_SIZE, PagesAsSlice, PhysAddr, PinnedHeapPages, VirtAddr,
        page_tables::{RootPageTableHolder, XWRMode},
    },
    processes::brk::Brk,
};

pub const STACK_START: VirtAddr = VirtAddr::new(usize::MAX);

pub const STACK_SIZE_PAGES: usize = 32;
pub const STACK_SIZE: usize = PAGE_SIZE * STACK_SIZE_PAGES;

pub const STACK_END: VirtAddr = VirtAddr::new(usize::MAX - STACK_SIZE + 1);

#[derive(Debug)]
pub struct LoadedElf {
    pub entry_address: VirtAddr,
    pub page_tables: RootPageTableHolder,
    pub allocated_pages: BTreeMap<VirtAddr, PinnedHeapPages>,
    pub args_start: VirtAddr,
    pub brk: Brk,
}

struct AuxvInfo {
    phdr_vaddr: usize,
    phent: usize,
    phnum: usize,
}

const AT_RANDOM_SIZE: usize = 16;

fn set_up_arguments(
    stack: &mut [u8],
    name: &str,
    args: &[&str],
    env: &[&str],
    auxv_info: &AuxvInfo,
) -> Result<VirtAddr, LoaderError> {
    // layout:
    // [argc, argv[0]..argv[n], NULL, envp[0]..envp[m], NULL, auxv..., name\0, args\0..., env\0..., random(16)]
    let argc = 1 + args.len(); // name + amount of args
    let mut argv = vec![0usize; args.len() + 2]; // number of args plus name and null terminator
    let mut envp = vec![0usize; env.len() + 1]; // env entries + NULL

    let mut random_bytes = [0u8; AT_RANDOM_SIZE];
    if crate::drivers::virtio::rng::is_available() {
        crate::drivers::virtio::rng::read_random(&mut random_bytes);
    }

    let mut auxv = [
        AT_PAGESZ as usize,
        PAGE_SIZE,
        AT_PHDR as usize,
        auxv_info.phdr_vaddr,
        AT_PHENT as usize,
        auxv_info.phent,
        AT_PHNUM as usize,
        auxv_info.phnum,
        AT_RANDOM as usize,
        0, // placeholder, patched below
        AT_NULL as usize,
        0,
    ];

    let strings = [name]
        .iter()
        .chain(args)
        .chain(env)
        .flat_map(|s| s.as_bytes().iter().chain(&[0]))
        .copied()
        .collect::<Vec<u8>>();

    let start_of_strings_offset =
        core::mem::size_of_val(&argc) + argv.in_bytes() + envp.in_bytes() + auxv.in_bytes();

    let random_bytes_offset = start_of_strings_offset + strings.in_bytes();
    let total_length = align_up(random_bytes_offset + AT_RANDOM_SIZE, 8);

    if total_length >= stack.len() {
        return Err(LoaderError::StackToSmall);
    }

    let real_start = STACK_START - total_length + 1;

    // Patch AT_RANDOM value to point at the random bytes on the stack.
    // Search only keys (even indices) to avoid matching a value that happens to equal AT_RANDOM.
    let at_random_pair_idx = auxv
        .chunks(2)
        .position(|pair| pair[0] == AT_RANDOM as usize)
        .expect("AT_RANDOM must be in auxv");
    auxv[at_random_pair_idx * 2 + 1] = (real_start + random_bytes_offset).as_usize();

    let mut addr_current_string = real_start + start_of_strings_offset;

    // Patch argv pointers
    argv[0] = addr_current_string.as_usize();
    addr_current_string =
        VirtAddr::new(addr_current_string.as_usize().wrapping_add(name.len() + 1));
    for (idx, arg) in args.iter().enumerate() {
        argv[idx + 1] = addr_current_string.as_usize();
        addr_current_string =
            VirtAddr::new(addr_current_string.as_usize().wrapping_add(arg.len() + 1));
    }

    // Patch envp pointers
    for (idx, e) in env.iter().enumerate() {
        envp[idx] = addr_current_string.as_usize();
        addr_current_string =
            VirtAddr::new(addr_current_string.as_usize().wrapping_add(e.len() + 1));
    }

    let offset = stack.len() - total_length;

    let mut writable_buffer = WritableBuffer::new(&mut stack[offset..]);

    writable_buffer
        .write_usize(argc)
        .map_err(|_| LoaderError::StackToSmall)?;

    for arg in argv {
        writable_buffer
            .write_usize(arg)
            .map_err(|_| LoaderError::StackToSmall)?;
    }

    for e in envp {
        writable_buffer
            .write_usize(e)
            .map_err(|_| LoaderError::StackToSmall)?;
    }

    for aux in auxv {
        writable_buffer
            .write_usize(aux)
            .map_err(|_| LoaderError::StackToSmall)?;
    }

    writable_buffer
        .write_slice(&strings)
        .map_err(|_| LoaderError::StackToSmall)?;

    writable_buffer
        .write_slice(&random_bytes)
        .map_err(|_| LoaderError::StackToSmall)?;

    // We want to point into the arguments
    Ok(STACK_START - total_length + 1)
}

pub fn load_elf(
    elf_file: &ElfFile,
    name: &str,
    args: &[&str],
    env: &[&str],
) -> Result<LoadedElf, LoaderError> {
    let mut page_tables = RootPageTableHolder::new_with_kernel_mapping(&[]);

    let elf_header = elf_file.get_header();
    let mut allocated_pages = BTreeMap::new();

    // Compute AT_PHDR: the virtual address where program headers are mapped.
    // The first LOAD segment maps from file offset 0 (containing the ELF header
    // and program headers), so AT_PHDR = first_load_vaddr + e_phoff.
    let first_load = elf_file
        .get_program_headers()
        .iter()
        .find(|h| h.header_type == ProgramHeaderType::PT_LOAD)
        .expect("ELF must have at least one LOAD segment");
    assert_eq!(
        first_load.offset_in_file, 0,
        "First LOAD segment must start at file offset 0 for AT_PHDR to be valid"
    );
    let auxv_info = AuxvInfo {
        phdr_vaddr: first_load.virtual_address.as_usize()
            + elf_header.start_program_header.as_usize(),
        phent: elf_header.size_program_header_entry as usize,
        phnum: elf_header.number_of_entries_in_program_header as usize,
    };

    let mut stack = PinnedHeapPages::new(STACK_SIZE_PAGES);

    let args_start = set_up_arguments(stack.as_u8_slice(), name, args, env, &auxv_info)?;

    let stack_addr = stack.addr();
    let prev = allocated_pages.insert(STACK_END, stack);
    assert!(prev.is_none(), "duplicate allocated_pages key for stack");

    debug!(
        "before mapping stack: stack_start={} stack_size={STACK_SIZE:#x} stack_end={}",
        STACK_START, STACK_END
    );

    page_tables.map_userspace(
        STACK_END,
        PhysAddr::new(stack_addr),
        STACK_SIZE,
        XWRMode::ReadWrite,
        "Stack".to_string(),
    );

    page_tables.map_userspace(
        super::signal::TRAMPOLINE_VADDR,
        super::signal::trampoline_phys_addr(),
        PAGE_SIZE,
        XWRMode::ReadExecute,
        "Signal trampoline".to_string(),
    );

    // Map load program header
    let loadable_program_header = || {
        elf_file
            .get_program_headers()
            .iter()
            .filter(|header| header.header_type == ProgramHeaderType::PT_LOAD)
    };

    for program_header in loadable_program_header() {
        debug!("Load {:#X?}", program_header);

        let data = elf_file.get_program_header_data(program_header);
        let real_size = program_header.memory_size;

        let real_size_usize = real_size.as_usize();
        assert!(
            real_size_usize >= data.len(),
            "real size must always be greater than the actual data"
        );

        let offset = program_header.virtual_address.as_usize() % PAGE_SIZE;

        let mut size_in_pages = minimum_amount_of_pages(real_size_usize);

        // Take into account when we spill into the next page
        size_in_pages += minimum_amount_of_pages(offset + real_size_usize)
            - minimum_amount_of_pages(real_size_usize);

        let mut pages = PinnedHeapPages::new(size_in_pages);

        debug!(
            "Allocated {size_in_pages} pages and fill at offset={offset:#X} with data.len={:#X}",
            data.len()
        );

        pages.fill(data, offset);

        let pages_addr = pages.addr();
        let mapping_va = VirtAddr::new(program_header.virtual_address.as_usize() - offset);

        let prev = allocated_pages.insert(mapping_va, pages);
        assert!(
            prev.is_none(),
            "duplicate allocated_pages key at {mapping_va}"
        );

        page_tables.map_userspace(
            mapping_va,
            PhysAddr::new(pages_addr),
            size_in_pages * PAGE_SIZE,
            program_header.access_flags.into(),
            "LOAD".to_string(),
        );
    }

    let bss_end = loadable_program_header()
        .map(|l| l.virtual_address + l.memory_size)
        .max();

    let brk = match bss_end {
        Some(bss_end) => {
            let (pages, brk) = Brk::new(VirtAddr::new(bss_end.as_usize()), &mut page_tables);
            let prev = allocated_pages.insert(brk.start(), pages);
            assert!(prev.is_none(), "duplicate allocated_pages key for brk");
            brk
        }
        None => Brk::empty(),
    };

    Ok(LoadedElf {
        entry_address: VirtAddr::new(elf_header.entry_point.as_usize()),
        page_tables,
        allocated_pages,
        args_start,
        brk,
    })
}
