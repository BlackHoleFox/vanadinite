// This Source Code Form is subject to the terms of the Mozilla Public License,
// v. 2.0. If a copy of the MPL was not distributed with this file, You can
// obtain one at https://mozilla.org/MPL/2.0/.

use crate::{BigEndianU32, BigEndianU64, Fdt};
use common::byteorder::FromBytes;

const FDT_BEGIN_NODE: u32 = 1;
const FDT_END_NODE: u32 = 2;
const FDT_PROP: u32 = 3;
const FDT_NOP: u32 = 4;
const FDT_END: u32 = 5;

#[derive(Debug, Clone, Copy)]
pub struct MemoryNode<'a> {
    pub(crate) node: FdtNode<'a>,
}

impl MemoryNode<'_> {
    pub fn regions(&self) -> impl Iterator<Item = MemoryRegion> + '_ {
        self.node.reg().unwrap()
    }

    pub fn initial_mapped_area(&self) -> Option<MappedArea> {
        let mut mapped_area = None;

        if let Some(init_mapped_area) = self.node.properties().find(|n| n.name == "initial_mapped_area") {
            let mut stream = common::byteorder::IntegerStream::new(init_mapped_area.value);
            let effective_address: BigEndianU64 = stream.next().expect("effective address");
            let physical_address: BigEndianU64 = stream.next().expect("physical address");
            let size: BigEndianU32 = stream.next().expect("size");

            mapped_area = Some(MappedArea {
                effective_address: effective_address.get() as usize,
                physical_address: physical_address.get() as usize,
                size: size.get() as usize,
            });
        }

        mapped_area
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    pub starting_address: *const u8,
    pub size: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct MappedArea {
    pub effective_address: usize,
    pub physical_address: usize,
    pub size: usize,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct FdtProperty {
    len: BigEndianU32,
    name_offset: BigEndianU32,
}

#[derive(Debug, Clone, Copy)]
pub struct FdtNode<'a> {
    pub name: &'a str,
    pub(crate) header: &'a Fdt,
    props: *const BigEndianU32,
    parent_props: Option<*const BigEndianU32>,
}

impl<'a> FdtNode<'a> {
    fn new(
        name: &'a str,
        header: &'a Fdt,
        props: *const BigEndianU32,
        parent_props: Option<*const BigEndianU32>,
    ) -> Self {
        Self { name, header, props, parent_props }
    }

    pub fn properties(self) -> impl Iterator<Item = NodeProperty<'a>> {
        let mut ptr = self.props;
        let mut done = false;

        core::iter::from_fn(move || {
            if done {
                return None;
            }

            if unsafe { *ptr }.get() == FDT_PROP {
                Some(NodeProperty::parse(&mut ptr, self.header))
            } else {
                done = true;
                None
            }
        })
    }

    pub fn children(self) -> impl Iterator<Item = FdtNode<'a>> {
        let mut ptr = self.props;

        while unsafe { *ptr }.get() == FDT_PROP {
            NodeProperty::parse(&mut ptr, self.header);
        }

        let mut done = false;

        core::iter::from_fn(move || {
            if done {
                return None;
            }

            while unsafe { *ptr }.get() == FDT_NOP {
                unsafe { advance_ptr(&mut ptr, 4) };
            }

            if unsafe { *ptr }.get() == FDT_BEGIN_NODE {
                let origin = ptr;
                let ret = unsafe {
                    advance_ptr(&mut ptr, 4);
                    let unit_name = cstr_core::CStr::from_ptr(ptr.cast()).to_str().expect("bad utf8");
                    advance_ptr(&mut ptr, unit_name.as_bytes().len() + 1);
                    let offset = ptr.cast::<u8>().align_offset(4);
                    advance_ptr(&mut ptr, offset);

                    Some(Self::new(unit_name, self.header, ptr, Some(self.props)))
                };

                ptr = origin;

                unsafe { skip_current_node(&mut ptr, self.header) };

                ret
            } else {
                done = true;
                None
            }
        })
    }

    /// Helper method for finding a `reg` property
    pub fn reg(self) -> Option<impl Iterator<Item = crate::MemoryRegion> + 'a> {
        let sizes = self.cell_sizes();
        if sizes.address_cells > 2 || sizes.size_cells > 2 {
            todo!("address-cells and size-cells > 2 u32s not supported yet");
        }

        let mut reg = None;
        for prop in self.properties() {
            if prop.name == "reg" {
                let mut stream = common::byteorder::IntegerStream::new(prop.value);
                reg = Some(core::iter::from_fn(move || {
                    let starting_address = match sizes.address_cells {
                        1 => stream.next::<BigEndianU32>()?.get() as usize,
                        2 => stream.next::<BigEndianU64>()?.get() as usize,
                        _ => return None,
                    } as *const u8;

                    let size = match sizes.size_cells {
                        0 => None,
                        1 => Some(stream.next::<BigEndianU32>()?.get() as usize),
                        2 => Some(stream.next::<BigEndianU64>()?.get() as usize),
                        _ => return None,
                    };

                    Some(MemoryRegion { starting_address, size })
                }));
                break;
            }
        }

        reg
    }

    /// Helper method for finding a `compatible` property
    pub fn compatible(self) -> Option<Compatible<'a>> {
        let mut s = None;
        for prop in self.properties() {
            if prop.name == "compatible" {
                s = Some(Compatible { data: prop.value });
            }
        }

        s
    }

    pub fn cell_sizes(self) -> CellSizes {
        let mut address_cells = None;
        let mut size_cells = None;

        for property in self.properties() {
            match property.name {
                "#address-cells" => address_cells = BigEndianU32::from_bytes(property.value).map(|n| n.get() as usize),
                "#size-cells" => size_cells = BigEndianU32::from_bytes(property.value).map(|n| n.get() as usize),
                _ => {}
            }
        }

        if let Some(parent) = self.parent_props {
            let parent = FdtNode { name: "", props: parent, header: self.header, parent_props: None };
            let parent_sizes = parent.cell_sizes();

            if address_cells.is_none() {
                address_cells = Some(parent_sizes.address_cells);
            }

            if size_cells.is_none() {
                size_cells = Some(parent_sizes.size_cells);
            }
        }

        // FIXME: this works around a bug(?) in the QEMU FDT
        if address_cells == Some(0) {
            address_cells = Some(2);
        }

        CellSizes { address_cells: address_cells.unwrap_or(2), size_cells: size_cells.unwrap_or(1) }
    }

    pub fn interrupt_parent(self) -> Option<FdtNode<'a>> {
        self.properties()
            .find(|p| p.name == "interrupt-parent")
            .and_then(|p| self.header.find_phandle(BigEndianU32::from_bytes(p.value)?.get()))
    }

    pub fn interrupt_cells(self) -> Option<usize> {
        let mut interrupt_cells = None;

        if let Some(prop) = self.properties().find(|p| p.name == "#interrupt-cells") {
            interrupt_cells = BigEndianU32::from_bytes(prop.value).map(|n| n.get() as usize)
        }

        if let (None, Some(parent)) = (interrupt_cells, self.interrupt_parent()) {
            interrupt_cells = parent.interrupt_cells();
        }

        interrupt_cells
    }

    /// Helper method for finding a `interrupts` property
    pub fn interrupts(self) -> Option<impl Iterator<Item = usize> + 'a> {
        let sizes = self.interrupt_cells()?;

        let mut interrupt = None;
        for prop in self.properties() {
            if prop.name == "interrupts" {
                let mut stream = common::byteorder::IntegerStream::new(prop.value);
                interrupt = Some(core::iter::from_fn(move || {
                    let interrupt = match sizes {
                        1 => stream.next::<BigEndianU32>()?.get() as usize,
                        2 => stream.next::<BigEndianU64>()?.get() as usize,
                        _ => return None,
                    };

                    Some(interrupt)
                }));
                break;
            }
        }

        interrupt
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CellSizes {
    pub address_cells: usize,
    pub size_cells: usize,
}

impl Default for CellSizes {
    fn default() -> Self {
        CellSizes { address_cells: 2, size_cells: 1 }
    }
}

#[derive(Clone, Copy)]
pub struct Compatible<'a> {
    data: &'a [u8],
}

impl<'a> Compatible<'a> {
    pub fn first(self) -> &'a str {
        let idx = self.data.iter().position(|b| *b == b'\0').unwrap_or(1) - 1;
        core::str::from_utf8(&self.data[..idx]).expect("valid utf-8")
    }

    pub fn all(self) -> impl Iterator<Item = &'a str> {
        let mut data = self.data;
        core::iter::from_fn(move || {
            if data.is_empty() {
                return None;
            }

            match data.iter().position(|b| *b == b'\0') {
                Some(idx) => {
                    let ret = Some(core::str::from_utf8(&data[..idx]).ok()?);
                    data = &data[idx + 1..];

                    ret
                }
                None => {
                    let ret = Some(core::str::from_utf8(data).ok()?);
                    data = &[];

                    ret
                }
            }
        })
    }
}

pub(crate) unsafe fn find_node<'a>(
    ptr: &mut *const BigEndianU32,
    name: &str,
    header: &'a Fdt,
    parent_props: Option<*const BigEndianU32>,
) -> Option<FdtNode<'a>> {
    let mut parts = name.splitn(2, '/');
    let looking_for = parts.next()?;

    while (**ptr).get() == FDT_NOP {
        advance_ptr(ptr, 4);
    }

    let node_ptr = *ptr;

    match (**ptr).get() {
        FDT_BEGIN_NODE => advance_ptr(ptr, 4),
        _ => return None,
    }

    let unit_name = cstr_core::CStr::from_ptr(ptr.cast()).to_str().ok()?;

    advance_ptr(ptr, unit_name.as_bytes().len() + 1);
    let offset = ptr.cast::<u8>().align_offset(4);
    advance_ptr(ptr, offset);

    let addr_name_same = looking_for.contains('@') && unit_name == looking_for;
    let base_name_same = unit_name.split('@').next()? == looking_for;

    if !addr_name_same && !base_name_same {
        *ptr = node_ptr;
        skip_current_node(ptr, header);

        return None;
    }

    let next_part = match parts.next() {
        None | Some("") => return Some(FdtNode::new(unit_name, header, *ptr, parent_props)),
        Some(part) => part,
    };

    while *ptr < header.structs_limit().cast() {
        let parent_props = Some(*ptr);

        while (**ptr).get() == FDT_PROP {
            let _ = NodeProperty::parse(ptr, header);
        }

        while (**ptr).get() == FDT_BEGIN_NODE {
            if let Some(p) = find_node(ptr, next_part, header, parent_props) {
                return Some(p);
            }
        }

        while (**ptr).get() == FDT_NOP {
            advance_ptr(ptr, 4);
        }

        if (**ptr).get() != FDT_END_NODE {
            return None;
        }

        advance_ptr(ptr, 4);
    }

    None
}

// FIXME: this probably needs refactored
pub(crate) unsafe fn all_nodes(header: &Fdt) -> impl Iterator<Item = FdtNode<'_>> {
    let mut ptr: *const BigEndianU32 = header.structs_ptr().cast();
    let mut done = false;
    let mut parents = [core::ptr::null(); 128];
    let mut parent_index = 0;

    core::iter::from_fn(move || {
        if done {
            return None;
        }

        while (*ptr).get() == FDT_END_NODE {
            parent_index -= 1;
            advance_ptr(&mut ptr, 4);
        }

        if (*ptr).get() == FDT_END {
            done = true;
            return None;
        }

        while (*ptr).get() == FDT_NOP {
            advance_ptr(&mut ptr, 4);
        }

        match (*ptr).get() {
            FDT_BEGIN_NODE => advance_ptr(&mut ptr, 4),
            _ => return None,
        }

        let unit_name = cstr_core::CStr::from_ptr(ptr.cast()).to_str().ok()?;

        advance_ptr(&mut ptr, unit_name.as_bytes().len() + 1);
        let offset = ptr.cast::<u8>().align_offset(4);
        advance_ptr(&mut ptr, offset);

        let node_ptr = ptr;

        parent_index += 1;
        parents[parent_index] = ptr;

        log::debug!(
            "hit node: {:?}, ptr: {:#p}, parent: {:?}",
            unit_name,
            ptr,
            match parent_index {
                1 => None,
                _ => Some(parents[parent_index - 1]),
            }
        );

        while (*ptr).get() == FDT_PROP {
            NodeProperty::parse(&mut ptr, header);
        }

        Some(FdtNode {
            name: if unit_name.is_empty() { "/" } else { unit_name },
            header,
            parent_props: match parent_index {
                1 => None,
                _ => Some(parents[parent_index - 1]),
            },
            props: node_ptr,
        })
    })
}

pub(crate) unsafe fn skip_current_node(ptr: &mut *const BigEndianU32, header: &Fdt) {
    assert_eq!((**ptr).get(), FDT_BEGIN_NODE, "bad node");
    advance_ptr(ptr, 4);

    let unit_name = cstr_core::CStr::from_ptr(ptr.cast()).to_str().ok().unwrap();
    advance_ptr(ptr, unit_name.as_bytes().len() + 1);
    let offset = ptr.cast::<u8>().align_offset(4);
    advance_ptr(ptr, offset);

    while (**ptr).get() == FDT_PROP {
        NodeProperty::parse(ptr, header);
    }

    while (**ptr).get() == FDT_BEGIN_NODE {
        skip_current_node(ptr, header);
    }

    while (**ptr).get() == FDT_NOP {
        advance_ptr(ptr, 4);
    }

    assert_eq!((**ptr).get(), FDT_END_NODE, "bad node");
    advance_ptr(ptr, 4);
}

#[derive(Debug, Clone, Copy)]
pub struct NodeProperty<'a> {
    pub name: &'a str,
    pub value: &'a [u8],
}

impl<'a> NodeProperty<'a> {
    pub fn as_usize(self) -> Option<usize> {
        match self.value.len() {
            4 => BigEndianU32::from_bytes(self.value).map(|i| i.get() as usize),
            8 => BigEndianU64::from_bytes(self.value).map(|i| i.get() as usize),
            _ => None,
        }
    }

    pub fn as_str(self) -> Option<&'a str> {
        core::str::from_utf8(self.value).ok()
    }

    fn parse(ptr: &mut *const BigEndianU32, header: &Fdt) -> Self {
        unsafe {
            if (**ptr).get() != FDT_PROP {
                panic!("bad prop");
            }

            advance_ptr(ptr, 4);

            let prop: FdtProperty = *ptr.cast();
            let data = ptr.cast::<u8>().add(core::mem::size_of::<FdtProperty>());
            advance_ptr(ptr, core::mem::size_of::<FdtProperty>() + prop.len.get() as usize);
            let offset = ptr.cast::<u8>().align_offset(4);
            advance_ptr(ptr, offset);

            NodeProperty {
                name: header.str_at_offset(prop.name_offset.get() as usize),
                value: core::slice::from_raw_parts(data, prop.len.get() as usize),
            }
        }
    }
}

pub(crate) unsafe fn advance_ptr<T>(ptr: &mut *const T, bytes: usize) {
    *ptr = ptr.cast::<u8>().add(bytes).cast();
}
