//! Windows-owned driver items. No registry or WIA application contexts are created.

use super::{E_INVALIDARG, E_POINTER};
use crate::com_server::Guid;
use std::{ffi::c_void, ptr};

const ROOT_FLAGS: i32 = 0x4c; // Folder | Root | Device
const FLATBED_FLAGS: i32 = 0x82003; // Image | File | Transfer | ProgrammableDataSource
const DISCONNECTED: i32 = 0x100;
const MAX_NAME_UNITS: usize = 16 * 1024;

#[repr(C)]
struct ItemVtable {
    query: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    flags: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
    context: unsafe extern "system" fn(*mut c_void, *mut *mut u8) -> i32,
    full_name: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32,
    name: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32,
    add_to_folder: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    unlink: unsafe extern "system" fn(*mut c_void, i32) -> i32,
    remove: unsafe extern "system" fn(*mut c_void, i32) -> i32,
    find: unsafe extern "system" fn(*mut c_void, i32, *mut u16, *mut *mut c_void) -> i32,
    find_child: unsafe extern "system" fn(*mut c_void, *mut u16, *mut *mut c_void) -> i32,
    parent: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    first_child: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    sibling: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
    dump: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32,
}

struct Item(*mut c_void);
impl Item {
    fn methods(&self) -> &ItemVtable {
        // SAFETY: wiasCreateDrvItem returned an owned live IWiaDrvItem reference.
        unsafe { &**self.0.cast::<*const ItemVtable>() }
    }
}
impl Drop for Item {
    fn drop(&mut self) {
        // SAFETY: balances this wrapper's one native item reference.
        unsafe {
            (self.methods().release)(self.0);
        }
    }
}

pub(super) struct Tree {
    root: Option<Item>,
}
impl Tree {
    /// Caller must keep the live native minidriver and COM apartment callable
    /// until close/drop, including all context-free callbacks made by Windows.
    pub(super) unsafe fn create(mini: *mut c_void, full_name: &[u16]) -> Result<Self, i32> {
        if mini.is_null()
            || full_name.is_empty()
            || full_name.len() + 8 > MAX_NAME_UNITS
            || full_name.contains(&0)
        {
            return Err(E_INVALIDARG);
        }
        let root_name = BString::new(&"Root".encode_utf16().collect::<Vec<_>>())?;
        let root_full = BString::new(full_name)?;
        // SAFETY: all strings and the borrowed minidriver remain live across creation.
        let root = unsafe { create_item(mini, ROOT_FLAGS, &root_name, &root_full) }?;
        let tree = Self { root: Some(root) };
        let flatbed_name = BString::new(&"Flatbed".encode_utf16().collect::<Vec<_>>())?;
        let mut child_full = full_name.to_vec();
        child_full.extend("\\Flatbed".encode_utf16());
        let child_full = BString::new(&child_full)?;
        // SAFETY: all strings and the borrowed minidriver remain live across creation.
        let child = unsafe { create_item(mini, FLATBED_FLAGS, &flatbed_name, &child_full) }?;
        #[cfg(test)]
        // SAFETY: child is owned; observe native reference count with a balanced pair.
        unsafe {
            (child.methods().add_ref)(child.0);
            assert_eq!((child.methods().release)(child.0), 1);
        }
        // SAFETY: both owned items remain live; the root has the folder flag.
        let hr = unsafe { (child.methods().add_to_folder)(child.0, tree.raw()) };
        if hr != 0 {
            return Err(hr);
        }
        #[cfg(test)]
        // SAFETY: child is owned; observe native reference count with a balanced pair.
        unsafe {
            (child.methods().add_ref)(child.0);
            assert_eq!((child.methods().release)(child.0), 2);
        }
        // The Windows tree retains linked children. Release our construction ref.
        drop(child);
        Ok(tree)
    }

    pub(super) fn raw(&self) -> *mut c_void {
        self.root.as_ref().map_or(ptr::null_mut(), |root| root.0)
    }

    pub(super) fn close(mut self) -> Result<(), i32> {
        self.disconnect()
    }

    fn disconnect(&mut self) -> Result<(), i32> {
        let Some(root) = self.root.take() else {
            return Ok(());
        };
        // SAFETY: the root is held until unlink completes, including callbacks.
        let hr = unsafe { (root.methods().unlink)(root.0, DISCONNECTED) };
        drop(root);
        if hr == 0 { Ok(()) } else { Err(hr) }
    }

    #[cfg(test)]
    pub(super) fn verify_flatbed(&self) -> Result<(), i32> {
        assert_eq!(std::mem::size_of::<ItemVtable>(), 128);
        assert_eq!(std::mem::offset_of!(ItemVtable, add_to_folder), 56);
        assert_eq!(std::mem::offset_of!(ItemVtable, unlink), 64);
        assert_eq!(std::mem::offset_of!(ItemVtable, first_child), 104);
        let root = self.root.as_ref().unwrap();
        // SAFETY: this test retains the root; native getter output is writable.
        unsafe {
            let mut flags = 0;
            assert_eq!((root.methods().flags)(root.0, &mut flags), 0);
            assert_eq!(flags, ROOT_FLAGS);
            assert_eq!(read_item_name(root, false), "Root");
            assert_eq!(read_item_name(root, true), "synthetic\\Root");
            let mut raw_child = ptr::null_mut();
            let hr = (root.methods().first_child)(root.0, &mut raw_child);
            if hr != 0 {
                return Err(hr);
            }
            if raw_child.is_null() {
                return Err(E_POINTER);
            }
            // GetFirstChildItem returns the tree's borrowed child on the tested
            // Windows implementation. Acquire our own reference before RAII.
            let child_methods = &**raw_child.cast::<*const ItemVtable>();
            (child_methods.add_ref)(raw_child);
            let child = Item(raw_child);
            (child.methods().add_ref)(child.0);
            assert_eq!((child.methods().release)(child.0), 2);
            assert_eq!((child.methods().flags)(child.0, &mut flags), 0);
            assert_eq!(flags, FLATBED_FLAGS);
            assert_eq!(read_item_name(&child, false), "Flatbed");
            assert_eq!(read_item_name(&child, true), "synthetic\\Root\\Flatbed");
        }
        Ok(())
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

unsafe fn create_item(
    mini: *mut c_void,
    flags: i32,
    name: &BString,
    full: &BString,
) -> Result<Item, i32> {
    let mut item = ptr::null_mut();
    // SAFETY: native IWiaMiniDrv and BSTR inputs are live. No device-specific
    // allocations are requested; Windows owns its internal item context.
    let hr =
        unsafe { wiasCreateDrvItem(flags, name.0, full.0, mini, 0, ptr::null_mut(), &mut item) };
    if hr != 0 {
        return Err(hr);
    }
    if item.is_null() {
        return Err(E_POINTER);
    }
    Ok(Item(item))
}

struct BString(*mut u16);
impl BString {
    fn new(words: &[u16]) -> Result<Self, i32> {
        if words.len() > MAX_NAME_UNITS {
            return Err(E_INVALIDARG);
        }
        // SAFETY: source covers words.len() initialized UTF-16 code units.
        let raw = unsafe { SysAllocStringLen(words.as_ptr(), words.len() as u32) };
        if raw.is_null() {
            Err(0x8007000eu32 as i32)
        } else {
            Ok(Self(raw))
        }
    }
}
impl Drop for BString {
    fn drop(&mut self) {
        // SAFETY: balances our owned allocation from OleAut32.
        unsafe {
            SysFreeString(self.0);
        }
    }
}

#[cfg(test)]
unsafe fn read_item_name(item: &Item, full: bool) -> String {
    let mut raw = ptr::null_mut();
    let method = if full {
        item.methods().full_name
    } else {
        item.methods().name
    };
    // SAFETY: native item returns an owned BSTR to valid output storage.
    unsafe {
        assert_eq!(method(item.0, &mut raw), 0);
        assert!(!raw.is_null());
        let owned = BString(raw);
        String::from_utf16(std::slice::from_raw_parts(
            owned.0,
            SysStringLen(owned.0) as usize,
        ))
        .unwrap()
    }
}

#[link(name = "Wiaservc")]
unsafe extern "system" {
    fn wiasCreateDrvItem(
        flags: i32,
        name: *mut u16,
        full: *mut u16,
        mini: *mut c_void,
        bytes: i32,
        context: *mut *mut u8,
        output: *mut *mut c_void,
    ) -> i32;
}
#[link(name = "OleAut32")]
unsafe extern "system" {
    fn SysAllocStringLen(words: *const u16, count: u32) -> *mut u16;
    fn SysFreeString(value: *mut u16);
    #[cfg(test)]
    fn SysStringLen(value: *mut u16) -> u32;
}
