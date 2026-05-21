/// ustar (POSIX tar) ramdisk parser.
///
/// Builds an in-memory tree from the bootloader-provided ustar archive.
/// File data is backed by slices into the ramdisk region — no copying.
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::str;

use crate::vfs::{NodeKind, VNode};

// ── ustar layout constants ────────────────────────────────────────────────────

const BLOCK:     usize = 512;
const NAME_OFF:  usize = 0;
const NAME_LEN:  usize = 100;
const SIZE_OFF:  usize = 124;
const SIZE_LEN:  usize = 12;
const TYPE_OFF:  usize = 156;
const MAGIC_OFF: usize = 257;
const MAGIC:     &[u8] = b"ustar";

fn parse_octal(bytes: &[u8]) -> usize {
    let s = str::from_utf8(bytes).unwrap_or("0");
    usize::from_str_radix(s.trim_matches('\0').trim(), 8).unwrap_or(0)
}

fn trim_nul(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

// ── Internal tree (built during parsing, before boxing) ──────────────────────

enum Node {
    File(&'static [u8]),
    Dir(Vec<(String, Node)>),
}

impl Node {
    fn as_dir_mut(&mut self) -> Option<&mut Vec<(String, Node)>> {
        match self {
            Node::Dir(v) => Some(v),
            _            => None,
        }
    }
}

fn get_or_insert_dir<'a>(
    entries: &'a mut Vec<(String, Node)>,
    name: &str,
) -> &'a mut Vec<(String, Node)> {
    let pos = entries.iter().position(|(n, _)| n == name);
    if pos.is_none() {
        entries.push((String::from(name), Node::Dir(Vec::new())));
    }
    let idx = entries.iter().position(|(n, _)| n == name).unwrap();
    entries[idx].1.as_dir_mut().expect("expected dir node")
}

fn insert(root: &mut Vec<(String, Node)>, path: &str, node: Node) {
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() { return; }

    let mut dir = root;
    for &part in &parts[..parts.len() - 1] {
        dir = get_or_insert_dir(dir, part);
    }

    let leaf = parts[parts.len() - 1];
    // Don't overwrite an existing dir entry with a dir placeholder.
    if matches!(node, Node::Dir(_))
        && dir.iter().any(|(n, _)| n == leaf)
    {
        return;
    }
    dir.push((String::from(leaf), node));
}

// ── Convert internal tree to VNode tree ──────────────────────────────────────

fn to_vnode(node: Node) -> Box<dyn VNode> {
    match node {
        Node::File(data)    => Box::new(RamfsFile { data }),
        Node::Dir(entries)  => Box::new(RamfsDir {
            entries: entries.into_iter()
                .map(|(name, child)| (name, to_vnode(child)))
                .collect(),
        }),
    }
}

// ── VNode implementations ─────────────────────────────────────────────────────

struct RamfsFile {
    data: &'static [u8],
}

impl VNode for RamfsFile {
    fn kind(&self) -> NodeKind { NodeKind::File }
    fn size(&self) -> usize    { self.data.len() }
    fn read(&self, buf: &mut [u8], offset: usize) -> usize {
        if offset >= self.data.len() { return 0; }
        let src = &self.data[offset..];
        let n   = buf.len().min(src.len());
        buf[..n].copy_from_slice(&src[..n]);
        n
    }
    fn lookup(&self, _: &str) -> Option<Box<dyn VNode>> { None }
}

struct RamfsDir {
    entries: Vec<(String, Box<dyn VNode>)>,
}

impl VNode for RamfsDir {
    fn kind(&self) -> NodeKind { NodeKind::Dir }
    fn size(&self) -> usize    { self.entries.len() }
    fn read(&self, _: &mut [u8], _: usize) -> usize { 0 }
    fn lookup(&self, name: &str) -> Option<Box<dyn VNode>> {
        self.entries.iter()
            .find(|(n, _)| n == name)
            .map(|(_, node)| {
                // Safety: the VFS tree lives in a 'static Mutex and is never
                // unmounted; all node references are effectively 'static.
                let node: &'static dyn VNode = unsafe { &*(node.as_ref() as *const dyn VNode) };
                vnode_clone(node)
            })
    }
}

/// Return a thin clone of any VNode (file: re-slice same data; dir: re-wrap entries).
fn vnode_clone(node: &'static dyn VNode) -> Box<dyn VNode> {
    match node.kind() {
        NodeKind::File => {
            let size = node.size();
            // Reconstruct: allocate a Vec, read all bytes, wrap as owned file.
            // For large files this could be memory-intensive, but our ramdisk
            // is tiny and files are path-looked-up rarely.
            let mut data = alloc::vec![0u8; size];
            node.read(&mut data, 0);
            Box::new(OwnedFile { data })
        }
        NodeKind::Dir => {
            // Return a thin pointer-wrapper; safe because the tree is 'static.
            Box::new(DirRef { ptr: node as *const dyn VNode })
        }
    }
}

/// File clone backed by owned bytes (used when returning from lookup).
struct OwnedFile { data: Vec<u8> }
impl VNode for OwnedFile {
    fn kind(&self) -> NodeKind { NodeKind::File }
    fn size(&self) -> usize    { self.data.len() }
    fn read(&self, buf: &mut [u8], offset: usize) -> usize {
        if offset >= self.data.len() { return 0; }
        let src = &self.data[offset..];
        let n   = buf.len().min(src.len());
        buf[..n].copy_from_slice(&src[..n]);
        n
    }
    fn lookup(&self, _: &str) -> Option<Box<dyn VNode>> { None }
}

/// Directory pointer wrapper — delegates through the static tree.
struct DirRef { ptr: *const dyn VNode }
unsafe impl Send for DirRef {}
unsafe impl Sync for DirRef {}
impl VNode for DirRef {
    fn kind(&self) -> NodeKind { NodeKind::Dir }
    fn size(&self) -> usize    { unsafe { (*self.ptr).size() } }
    fn read(&self, _: &mut [u8], _: usize) -> usize { 0 }
    fn lookup(&self, name: &str) -> Option<Box<dyn VNode>> {
        unsafe { (*self.ptr).lookup(name) }
    }
}

// ── Public init ───────────────────────────────────────────────────────────────

pub fn init(ramdisk_addr: u64, ramdisk_len: usize, phys_off: usize) {
    if ramdisk_len == 0 {
        crate::serial_println!("[RAMFS] no ramdisk provided");
        return;
    }

    // Try ramdisk_addr directly as a virtual address (validated by ustar magic).
    // If that fails, treat it as physical and add phys_offset.
    let slice = try_slice(ramdisk_addr as usize, ramdisk_len)
        .or_else(|| try_slice(ramdisk_addr as usize + phys_off, ramdisk_len));

    let (slice, virt) = match slice {
        Some(v) => v,
        None => {
            crate::serial_println!("[RAMFS] ERROR: no valid ustar magic found");
            return;
        }
    };

    crate::serial_println!(
        "[RAMFS] ramdisk at virt {:#010x} ({} bytes)",
        virt, ramdisk_len
    );

    let root = parse_ustar(slice);
    crate::vfs::mount(to_vnode(Node::Dir(root)));
}

fn try_slice(virt: usize, len: usize) -> Option<(&'static [u8], usize)> {
    if len < BLOCK + MAGIC.len() { return None; }
    let slice: &'static [u8] = unsafe { core::slice::from_raw_parts(virt as *const u8, len) };
    if &slice[MAGIC_OFF..MAGIC_OFF + MAGIC.len()] == MAGIC {
        Some((slice, virt))
    } else {
        None
    }
}

fn parse_ustar(data: &'static [u8]) -> Vec<(String, Node)> {
    let mut root: Vec<(String, Node)> = Vec::new();
    let mut offset    = 0;
    let mut file_count = 0;

    while offset + BLOCK <= data.len() {
        let header = &data[offset..offset + BLOCK];

        if header.iter().all(|&b| b == 0) { break; } // end-of-archive
        if &header[MAGIC_OFF..MAGIC_OFF + MAGIC.len()] != MAGIC { break; }

        let name  = str::from_utf8(trim_nul(&header[NAME_OFF..NAME_OFF + NAME_LEN]))
            .unwrap_or("").trim_matches('/');
        let size  = parse_octal(&header[SIZE_OFF..SIZE_OFF + SIZE_LEN]);
        let ttype = header[TYPE_OFF];

        offset += BLOCK;

        if !name.is_empty() && name != "." {
            let data_slice = if size > 0 && offset + size <= data.len() {
                &data[offset..offset + size]
            } else {
                &[][..]
            };

            let node = if ttype == b'5' {
                Node::Dir(Vec::new())
            } else {
                file_count += 1;
                Node::File(data_slice)
            };

            insert(&mut root, name, node);
        }

        offset += (size + BLOCK - 1) / BLOCK * BLOCK;
    }

    crate::serial_println!("[RAMFS] mounted: {} file(s) in ramdisk", file_count);
    root
}
