// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.

pub mod heap;
pub mod phys;
pub mod paging {
    mod manager;
    mod perms;
    mod sv39;

    pub use manager::*;
    pub use perms::*;
    pub use sv39::*;
}

#[inline(always)]
pub fn sfence() {
    unsafe { asm!("sfence.vma") };
}

#[inline(always)]
pub fn fence() {
    unsafe { asm!("fence") };
}

#[inline(always)]
pub fn satp(mode: SatpMode, asid: u16, root_page_table: paging::PhysicalAddress) {
    let value = ((mode as usize) << 60) | ((asid as usize) << 44) | root_page_table.ppn();
    unsafe { asm!("csrw satp, {}", in(reg) value) };
}

#[repr(usize)]
pub enum SatpMode {
    Bare = 0,
    Sv39 = 8,
    Sv48 = 9,
}

pub mod kernel_patching {
    use crate::mem::paging::{PhysicalAddress, VirtualAddress};
    use crate::utils;
    use core::cell::UnsafeCell;

    extern "C" {
        static KERNEL_START: utils::LinkerSymbol;
        static KERNEL_END: utils::LinkerSymbol;
        static PAGE_OFFSET_VALUE: usize;
    }

    #[repr(transparent)]
    struct StaticUsize(UnsafeCell<usize>);

    unsafe impl Send for StaticUsize {}
    unsafe impl Sync for StaticUsize {}

    #[no_mangle]
    static KERNEL_PHYS_LOAD_LOCATION: StaticUsize = StaticUsize(UnsafeCell::new(0));

    pub fn phys2virt(phys: PhysicalAddress) -> VirtualAddress {
        let phys_offset = unsafe { *KERNEL_PHYS_LOAD_LOCATION.0.get() };

        assert!(phys_offset != 0);

        VirtualAddress::new(phys.as_usize() - phys_offset + page_offset())
    }

    pub fn virt2phys(virt: VirtualAddress) -> PhysicalAddress {
        let phys_offset = unsafe { *KERNEL_PHYS_LOAD_LOCATION.0.get() };

        assert!(phys_offset != 0);

        PhysicalAddress::new(virt.as_usize() - page_offset() + phys_offset)
    }

    #[inline(always)]
    pub fn page_offset() -> usize {
        unsafe { PAGE_OFFSET_VALUE }
    }

    pub fn kernel_start() -> *const u8 {
        unsafe { KERNEL_START.as_ptr() }
    }

    pub fn kernel_end() -> *const u8 {
        unsafe { KERNEL_END.as_ptr() }
    }
}
