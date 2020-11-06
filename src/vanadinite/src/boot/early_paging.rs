use crate::mem::{
    kernel_patching,
    paging::{
        Execute, PageSize, PhysicalAddress, Read, StaticPageTable, Sv39PageTable, VirtualAddress, Write,
        PAGE_TABLE_ROOT,
    },
    phys::{PhysicalMemoryAllocator, PHYSICAL_MEMORY_ALLOCATOR},
};
use core::cell::UnsafeCell;

const TWO_MEBS: usize = 2 * 1024 * 1024;

pub static TEMP_PAGE_TABLE_HIGH_MEM: StaticPageTable = StaticPageTable::new(UnsafeCell::new(Sv39PageTable::new()));
pub static TEMP_PAGE_TABLE_IDENTITY: StaticPageTable = StaticPageTable::new(UnsafeCell::new(Sv39PageTable::new()));

/// # Safety
/// no
#[no_mangle]
pub unsafe extern "C" fn early_paging(hart_id: usize, fdt: *const u8, phys_load: usize) -> ! {
    let kmain: usize;
    asm!("
        lla {tmp}, kmain_addr_virt
        ld {tmp}, ({tmp})
        mv {}, {tmp}
    ", out(reg) kmain, tmp = out(reg) _);

    let fdt_struct = match fdt::Fdt::new(fdt) {
        Some(fdt) => fdt,
        None => crate::arch::exit(crate::arch::ExitStatus::Error(&"magic's fucked, my dude")),
    };

    let fdt_size = fdt_struct.total_size() as u64;

    let page_offset_value = kernel_patching::page_offset();
    // These are physical addresses before paging is enabled
    let kernel_start = kernel_patching::kernel_start() as usize;
    let kernel_end = kernel_patching::kernel_end() as usize;

    let memory_region = fdt_struct
        .memory()
        .regions
        .iter()
        .copied()
        .find(|region| {
            let start = region.starting_address() as usize;
            let end = (region.starting_address() + region.size()) as usize;

            start <= kernel_start && kernel_start <= end
        })
        .expect("wtf");

    let start = memory_region.starting_address();
    let size = memory_region.size();

    let high_mem_phys = (TEMP_PAGE_TABLE_HIGH_MEM.get(), PhysicalAddress::from_ptr(&TEMP_PAGE_TABLE_HIGH_MEM));
    let ident_mem_phys = (TEMP_PAGE_TABLE_IDENTITY.get(), PhysicalAddress::from_ptr(&TEMP_PAGE_TABLE_IDENTITY));

    let root_page_table = &mut *PAGE_TABLE_ROOT.get();

    for address in (kernel_start..kernel_end).step_by(TWO_MEBS) {
        let virt = VirtualAddress::new(page_offset_value + (address - kernel_start));
        let ident = VirtualAddress::new(address);
        let phys = PhysicalAddress::new(address);
        let permissions = Read | Write | Execute;

        root_page_table.map(
            phys,
            virt,
            PageSize::Megapage,
            permissions,
            || high_mem_phys,
            |p| VirtualAddress::new(p.as_usize()),
        );

        root_page_table.map(
            phys,
            ident,
            PageSize::Megapage,
            permissions,
            || ident_mem_phys,
            |p| VirtualAddress::new(p.as_usize()),
        );
    }

    let fdt_usize = fdt as usize;
    let fdt_phys = PhysicalAddress::new(fdt_usize);
    let fdt_virt = VirtualAddress::new(fdt_usize);

    let alloc_start = if fdt_usize >= kernel_end {
        // round up to next page size after the FDT
        ((kernel_patching::phys2virt(fdt_phys).as_usize() + fdt_size as usize) & !0x1FFFFFusize) + 0x200000
    } else {
        kernel_end
    };

    if !root_page_table.is_mapped(VirtualAddress::new(alloc_start), |p| VirtualAddress::new(p.as_usize())) {
        let alloc_start = VirtualAddress::new(alloc_start);
        root_page_table.map(
            kernel_patching::virt2phys(alloc_start),
            alloc_start,
            PageSize::Megapage,
            Read | Write | Execute,
            || high_mem_phys,
            |p| VirtualAddress::new(p.as_usize()),
        );
    }

    // Set the allocator to start allocating memory after the end of the kernel or fdt
    let kernel_end_phys = kernel_patching::virt2phys(VirtualAddress::new(alloc_start)).as_mut_ptr();

    let mut pf_alloc = PHYSICAL_MEMORY_ALLOCATOR.lock();
    pf_alloc.init(kernel_end_phys, (start + size) as *mut u8);

    let mut page_alloc = || {
        let phys_addr = pf_alloc.alloc().unwrap().as_phys_address();
        //let virt_addr = kernel_patching::phys2virt(phys_addr);

        (phys_addr.as_mut_ptr() as *mut Sv39PageTable, phys_addr)
    };

    #[cfg(feature = "virt")]
    root_page_table.map(
        PhysicalAddress::new(0x10_0000),
        VirtualAddress::new(0x10_0000),
        PageSize::Kilopage,
        Read | Write,
        &mut page_alloc,
        |p| VirtualAddress::new(p.as_usize()),
    );

    if !root_page_table.is_mapped(fdt_virt, |p| VirtualAddress::new(p.as_usize())) {
        root_page_table
            .map(fdt_phys, fdt_virt, PageSize::Kilopage, Read, &mut page_alloc, |p| VirtualAddress::new(p.as_usize()));
    }

    drop(pf_alloc);

    let sp: usize;
    let gp: usize;
    asm!("lla {}, __tmp_stack_top", out(reg) sp);
    asm!("lla {}, __global_pointer$", out(reg) gp);

    let new_sp = (sp - phys_load) + page_offset_value;
    let new_gp = (gp - phys_load) + page_offset_value;

    crate::mem::satp(crate::mem::SatpMode::Sv39, 0, PhysicalAddress::from_ptr(&PAGE_TABLE_ROOT));
    crate::mem::sfence();

    vmem_trampoline(hart_id, fdt, new_sp, new_gp, kmain)
}

extern "C" {
    fn vmem_trampoline(_: usize, _: *const u8, sp: usize, gp: usize, dest: usize) -> !;
}

#[rustfmt::skip]
global_asm!("
    .section .text
    .globl vmem_trampoline
    vmem_trampoline:
        mv sp, a2
        mv gp, a3
        jr a4
");