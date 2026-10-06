//! A small, pure-Rust, read-only HDF5 reader.
//!
//! Written from the HDF5 File Format Specification rather than pulled in as
//! a crate: the `hdf5-metno` binding behind `save_model` needs a C library
//! and a cmake build, which the Windows/macOS/Linux/arm64 release matrix
//! cannot assume, and no maintained pure-Rust HDF5 reader was vetted for
//! this repository. So this covers the subset that real scientific files
//! overwhelmingly use, and refuses the rest **by name** -- an unsupported
//! feature is an error that says which feature, never wrong data.
//!
//! Supported:
//! * superblock versions 0-3 (also behind a user block, as in MATLAB v7.3);
//! * old-style groups (symbol table: v1 B-tree + SNOD + local heap) and
//!   new-style groups holding link messages in the object header (hard and
//!   soft links);
//! * object header versions 1 and 2, with continuation blocks;
//! * dataspace (v1/v2), datatype and data layout (v3, plus v4 compact /
//!   contiguous / single-chunk) messages, fill values;
//! * compact, contiguous and chunked (v1 chunk B-tree) storage, with the
//!   deflate, shuffle and fletcher32 filters (fletcher32's checksum is
//!   stripped, not verified);
//! * fixed-point (1/2/4/8 bytes), IEEE floating point (16/32/64), enum
//!   (read as its integer base), fixed-length and variable-length strings.
//!
//! Refused by name: compound, array, opaque, bitfield and reference
//! datatypes; dense link storage (fractal heap / v2 B-tree groups);
//! layout messages v1/v2 and v4 chunk indexes other than "single chunk";
//! shared object-header messages; external links; every filter but the
//! three above (szip, scale-offset, n-bit, LZF, Blosc, ...).
//!
//! The file is untrusted. Every offset and length is bounds-checked against
//! the file, every allocation is sized from a checked product and capped,
//! every graph walk (B-trees, continuation blocks, groups) has a visit
//! limit so a cyclic file terminates, and metadata checksums are not
//! trusted for anything.

use crate::inflate::zlib_decompress_capped;
use crate::matfile::MatValue;
use std::collections::{HashMap, HashSet};

type Res<T> = Result<T, String>;

/// Largest dataset (in bytes of raw data) that will be materialised.
pub const MAX_DATA_BYTES: usize = 1 << 30;
const MAX_OBJECTS: usize = 100_000;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 1_000_000;
/// Largest total of decoded variable-length string bytes in one dataset.
const MAX_TEXT_BYTES: usize = 256 << 20;
const MAX_RANK: usize = 32;
const MAX_NAME: usize = 4096;

const SIGNATURE: &[u8; 8] = b"\x89HDF\r\n\x1a\n";

// ------------------------------------------------------------ small helpers

/// Little-endian unsigned integer of `n` (1..=8) bytes at `at` of `d`.
fn le(d: &[u8], at: usize, n: usize) -> Res<u64> {
    if n == 0 || n > 8 {
        return Err(format!("unsupported integer width {n}"));
    }
    let end = at.checked_add(n).ok_or("offset overflow")?;
    let s = d.get(at..end).ok_or("truncated HDF5 structure (read past its end)")?;
    let mut v = 0u64;
    for (i, &b) in s.iter().enumerate() {
        v |= (b as u64) << (8 * i);
    }
    Ok(v)
}

fn cstr(d: &[u8]) -> String {
    let end = d.iter().position(|&b| b == 0).unwrap_or(d.len());
    String::from_utf8_lossy(&d[..end]).into_owned()
}

fn checked_product(dims: &[usize]) -> Res<usize> {
    let mut n = 1usize;
    for &d in dims {
        n = n.checked_mul(d).ok_or_else(|| format!("shape {dims:?} overflows"))?;
    }
    Ok(n)
}

// ------------------------------------------------------------------ types

#[derive(Debug, Clone, PartialEq)]
pub enum H5Type {
    Fixed { size: usize, signed: bool, big: bool },
    Float { size: usize, big: bool },
    /// Fixed-length string of `size` bytes.
    FixedStr { size: usize, utf8: bool },
    /// Variable-length string (global heap).
    VlenStr,
    /// Enumeration, read as its integer base type.
    Enum(Box<H5Type>),
    /// Anything else; carries what to call it in an error.
    Unsupported(String),
}

impl H5Type {
    fn size(&self, so: usize) -> Option<usize> {
        match self {
            H5Type::Fixed { size, .. } | H5Type::Float { size, .. } => Some(*size),
            H5Type::FixedStr { size, .. } => Some(*size),
            H5Type::VlenStr => Some(4 + so + 4),
            H5Type::Enum(b) => b.size(so),
            H5Type::Unsupported(_) => None,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            H5Type::Fixed { size, signed, .. } => {
                format!("{}int{}", if *signed { "" } else { "u" }, size * 8)
            }
            H5Type::Float { size, .. } => format!("float{}", size * 8),
            H5Type::FixedStr { size, .. } => format!("string[{size}]"),
            H5Type::VlenStr => "string".into(),
            H5Type::Enum(b) => format!("enum({})", b.describe()),
            H5Type::Unsupported(s) => format!("unsupported({s})"),
        }
    }
}

/// What a dataset read gives back before conversion to a `Value`. Real data
/// is in C (row-major) order, as stored.
#[derive(Debug, Clone, PartialEq)]
pub enum H5Value {
    Real { shape: Vec<usize>, data: Vec<f64> },
    Text { shape: Vec<usize>, data: Vec<String> },
}

/// One row of `h5info`.
#[derive(Debug, Clone, PartialEq)]
pub struct H5Entry {
    pub path: String,
    /// `group`, `dataset`, `datatype`, `softlink` or `externallink`.
    pub kind: &'static str,
    pub dtype: String,
    pub shape: Vec<usize>,
}

#[derive(Debug)]
struct Msg<'a> {
    ty: u16,
    flags: u8,
    data: &'a [u8],
}

enum LinkTarget {
    Hard(usize),
    Soft(String),
    External,
}
struct Link {
    name: String,
    target: LinkTarget,
}

#[derive(Debug, Clone)]
enum Layout {
    Compact(Vec<u8>),
    Contiguous { addr: Option<usize>, size: usize },
    Chunked { btree: usize, chunk: Vec<usize> },
    SingleChunk { addr: Option<usize>, chunk: Vec<usize>, filtered_size: Option<usize>, mask: u32 },
}

struct Filter {
    id: u16,
    cd: Vec<u32>,
}

struct Dataset {
    shape: Vec<usize>,
    ty: H5Type,
    layout: Layout,
    filters: Vec<Filter>,
    fill: Option<Vec<u8>>,
}

// ------------------------------------------------------------------- file

pub struct Hdf5<'a> {
    b: &'a [u8],
    /// File position of the superblock; all stored addresses are relative to it.
    base: usize,
    so: usize,
    sl: usize,
    root: usize,
}

/// Does this look like an HDF5 file (signature at 0, 512, 1024, ...)?
pub fn is_hdf5(bytes: &[u8]) -> bool {
    find_signature(bytes).is_some()
}

fn find_signature(b: &[u8]) -> Option<usize> {
    let mut pos = 0usize;
    while pos + 8 <= b.len() {
        if &b[pos..pos + 8] == SIGNATURE {
            return Some(pos);
        }
        pos = if pos == 0 { 512 } else { pos.checked_mul(2)? };
    }
    None
}

impl<'a> Hdf5<'a> {
    pub fn open(bytes: &'a [u8]) -> Res<Self> {
        let base = find_signature(bytes)
            .ok_or("not an HDF5 file (no HDF5 signature at offset 0, 512, 1024, ...)")?;
        let sb = &bytes[base..];
        let ver = *sb.get(8).ok_or("truncated HDF5 superblock")?;
        let (so, sl, root_off);
        match ver {
            0 | 1 => {
                so = le(sb, 13, 1)? as usize;
                sl = le(sb, 14, 1)? as usize;
                check_widths(so, sl)?;
                let addrs = if ver == 0 { 24 } else { 28 };
                // root symbol table entry follows the four file addresses
                let ste = addrs + 4 * so;
                root_off = le(sb, ste + so, so)?;
            }
            2 | 3 => {
                so = le(sb, 9, 1)? as usize;
                sl = le(sb, 10, 1)? as usize;
                check_widths(so, sl)?;
                root_off = le(sb, 12 + 3 * so, so)?;
            }
            v => return Err(format!("unsupported HDF5 superblock version {v}")),
        }
        let mut f = Hdf5 { b: bytes, base, so, sl, root: 0 };
        f.root = f
            .addr(root_off)?
            .ok_or("HDF5 root group address is undefined (empty or corrupt file)")?;
        Ok(f)
    }

    /// Map a stored address to a file position, or `None` if it is the
    /// "undefined address" (all ones).
    fn addr(&self, raw: u64) -> Res<Option<usize>> {
        let undef = if self.so == 8 { u64::MAX } else { (1u64 << (8 * self.so)) - 1 };
        if raw == undef {
            return Ok(None);
        }
        let pos = (self.base as u64)
            .checked_add(raw)
            .filter(|&p| p < self.b.len() as u64)
            .ok_or_else(|| format!("address {raw:#x} points past the end of the file"))?;
        Ok(Some(pos as usize))
    }

    fn rd(&self, at: usize, n: usize) -> Res<&'a [u8]> {
        let end = at.checked_add(n).ok_or("offset overflow")?;
        self.b.get(at..end).ok_or_else(|| {
            format!("truncated file: wanted {n} bytes at offset {at} but the file ends at {}", self.b.len())
        })
    }
    fn uint(&self, at: usize, n: usize) -> Res<u64> {
        le(self.b, at, n)
    }
    fn off(&self, at: usize) -> Res<u64> {
        le(self.b, at, self.so)
    }
    fn len_at(&self, at: usize) -> Res<u64> {
        le(self.b, at, self.sl)
    }
    /// Read a stored address at `at` and map it; `None` if undefined.
    fn addr_at(&self, at: usize) -> Res<Option<usize>> {
        self.addr(self.off(at)?)
    }
    fn need_addr(&self, at: usize, what: &str) -> Res<usize> {
        self.addr_at(at)?.ok_or_else(|| format!("{what} has an undefined address"))
    }

    // ------------------------------------------------------ object headers

    fn header(&self, at: usize) -> Res<Vec<Msg<'a>>> {
        let mut msgs: Vec<Msg<'a>> = Vec::new();
        let mut queue: Vec<(usize, usize)> = Vec::new(); // (start, end) of message area
        let v2 = self.rd(at, 4)? == b"OHDR";
        let mut creation_order = false;
        if v2 {
            let ver = self.uint(at + 4, 1)?;
            if ver != 2 {
                return Err(format!("unsupported object header version {ver}"));
            }
            let flags = self.uint(at + 5, 1)? as u8;
            let mut p = at + 6;
            if flags & 0x20 != 0 {
                p += 16;
            }
            if flags & 0x10 != 0 {
                p += 4;
            }
            let wlen = 1usize << (flags & 3);
            let csize = self.uint(p, wlen)? as usize;
            p += wlen;
            creation_order = flags & 0x04 != 0;
            let end = p.checked_add(csize).filter(|&e| e <= self.b.len()).ok_or("object header chunk runs past the end of the file")?;
            queue.push((p, end));
        } else {
            let ver = self.uint(at, 1)?;
            if ver != 1 {
                return Err(format!(
                    "unsupported object header version {ver} (is this really an HDF5 object?)"
                ));
            }
            let hsize = self.uint(at + 8, 4)? as usize;
            let start = at + 16;
            let end = start.checked_add(hsize).filter(|&e| e <= self.b.len()).ok_or("object header runs past the end of the file")?;
            queue.push((start, end));
        }
        let mut seen_blocks: HashSet<usize> = HashSet::new();
        let mut chunks = 0usize;
        while let Some((start, end)) = queue.pop() {
            chunks += 1;
            if chunks > 4096 {
                return Err("object header has too many continuation blocks".into());
            }
            let mut pos = start;
            let hdr = if v2 { 4 + if creation_order { 2 } else { 0 } } else { 8 };
            while pos + hdr <= end {
                let (ty, sz, flags) = if v2 {
                    (self.uint(pos, 1)? as u16, self.uint(pos + 1, 2)? as usize, self.uint(pos + 3, 1)? as u8)
                } else {
                    (self.uint(pos, 2)? as u16, self.uint(pos + 2, 2)? as usize, self.uint(pos + 4, 1)? as u8)
                };
                let dstart = pos + hdr;
                let dend = dstart.checked_add(sz).filter(|&e| e <= end).ok_or("object header message runs past its block")?;
                let data = &self.b[dstart..dend];
                if ty == 0x10 {
                    // continuation: offset, length
                    let off = self.addr(le(data, 0, self.so)?)?.ok_or("continuation block has an undefined address")?;
                    let len = le(data, self.so, self.sl)? as usize;
                    if !seen_blocks.insert(off) {
                        return Err("object header continuation blocks form a cycle or repeat".into());
                    }
                    let cend = off.checked_add(len).filter(|&e| e <= self.b.len()).ok_or("continuation block runs past the end of the file")?;
                    if v2 {
                        if self.rd(off, 4)? != b"OCHK" {
                            return Err("bad object header continuation block signature".into());
                        }
                        if len < 8 {
                            return Err("object header continuation block too short".into());
                        }
                        queue.push((off + 4, cend - 4));
                    } else {
                        queue.push((off, cend));
                    }
                } else if ty != 0 {
                    msgs.push(Msg { ty, flags, data });
                    if msgs.len() > 65_536 {
                        return Err("object header has too many messages".into());
                    }
                }
                pos = dend;
            }
        }
        for m in &msgs {
            if m.flags & 0x02 != 0 {
                return Err(format!(
                    "shared object-header messages (message type {:#x}) are not supported",
                    m.ty
                ));
            }
        }
        Ok(msgs)
    }

    // ------------------------------------------------------------- groups

    /// Links of the group described by `msgs`, or `None` if it is not a group.
    fn links(&self, msgs: &[Msg<'a>]) -> Res<Option<Vec<Link>>> {
        let mut out = Vec::new();
        let mut is_group = false;
        for m in msgs {
            match m.ty {
                0x11 => {
                    is_group = true;
                    let bt = le(m.data, 0, self.so)?;
                    let hp = le(m.data, self.so, self.so)?;
                    let (Some(bt), Some(hp)) = (self.addr(bt)?, self.addr(hp)?) else {
                        continue; // empty old-style group
                    };
                    self.symbol_table(bt, hp, &mut out)?;
                }
                0x02 => {
                    is_group = true;
                    let d = m.data;
                    let flags = le(d, 1, 1)?;
                    let mut p = 2;
                    if flags & 1 != 0 {
                        p += 8;
                    }
                    if self.addr(le(d, p, self.so)?)?.is_some() {
                        return Err("this group uses dense link storage (fractal heap), which is not supported; \
                                    re-save the file with libver='earliest' or fewer than 8 links per group"
                            .into());
                    }
                }
                0x06 => {
                    is_group = true;
                    out.push(self.link_message(m.data)?);
                }
                _ => {}
            }
        }
        Ok(if is_group { Some(out) } else { None })
    }

    fn link_message(&self, d: &[u8]) -> Res<Link> {
        let ver = le(d, 0, 1)?;
        if ver != 1 {
            return Err(format!("unsupported link message version {ver}"));
        }
        let flags = le(d, 1, 1)? as u8;
        let mut p = 2;
        let mut ty = 0u64;
        if flags & 0x08 != 0 {
            ty = le(d, p, 1)?;
            p += 1;
        }
        if flags & 0x04 != 0 {
            p += 8;
        }
        if flags & 0x10 != 0 {
            p += 1;
        }
        let wlen = 1usize << (flags & 3);
        let nlen = le(d, p, wlen)? as usize;
        p += wlen;
        if nlen > MAX_NAME {
            return Err("link name is implausibly long".into());
        }
        let name = String::from_utf8_lossy(d.get(p..p + nlen).ok_or("truncated link message")?).into_owned();
        p += nlen;
        let target = match ty {
            0 => LinkTarget::Hard(self.need_addr_raw(le(d, p, self.so)?, "hard link")?),
            1 => {
                let l = le(d, p, 2)? as usize;
                LinkTarget::Soft(String::from_utf8_lossy(d.get(p + 2..p + 2 + l).ok_or("truncated soft link")?).into_owned())
            }
            64 => LinkTarget::External,
            t => return Err(format!("unsupported link type {t}")),
        };
        Ok(Link { name, target })
    }

    fn need_addr_raw(&self, raw: u64, what: &str) -> Res<usize> {
        self.addr(raw)?.ok_or_else(|| format!("{what} has an undefined address"))
    }

    fn symbol_table(&self, btree: usize, heap: usize, out: &mut Vec<Link>) -> Res<()> {
        // local heap
        if self.rd(heap, 4)? != b"HEAP" {
            return Err("bad local heap signature".into());
        }
        let hsize = self.len_at(heap + 8)? as usize;
        let hdata = self.need_addr(heap + 8 + 2 * self.sl, "local heap data")?;
        let hbytes = self.rd(hdata, hsize.min(self.b.len().saturating_sub(hdata)))?;

        let mut stack = vec![(btree, 0usize)];
        let mut nodes = 0usize;
        let mut seen_nodes: HashSet<usize> = HashSet::new();
        let mut seen_snods: HashSet<usize> = HashSet::new();
        while let Some((node, depth)) = stack.pop() {
            nodes += 1;
            if nodes > MAX_NODES || depth > MAX_DEPTH {
                return Err("group B-tree is too deep or cyclic".into());
            }
            if !seen_nodes.insert(node) {
                return Err("group B-tree revisits a node (cycle or shared node)".into());
            }
            if self.rd(node, 4)? != b"TREE" {
                return Err("bad group B-tree node signature".into());
            }
            if self.uint(node + 4, 1)? != 0 {
                return Err("group B-tree node has the wrong node type".into());
            }
            let level = self.uint(node + 5, 1)?;
            let used = self.uint(node + 6, 2)? as usize;
            // keys (sL) and children (sO) alternate after the 8+2*so header
            let mut p = node + 8 + 2 * self.so + self.sl; // skip key 0
            for _ in 0..used {
                let child = self.need_addr(p, "group B-tree child")?;
                p += self.so + self.sl;
                if level > 0 {
                    stack.push((child, depth + 1));
                } else {
                    if !seen_snods.insert(child) {
                        return Err("group B-tree points several entries at one symbol table node".into());
                    }
                    self.snod(child, hbytes, out)?;
                }
            }
        }
        Ok(())
    }

    fn snod(&self, at: usize, heap: &[u8], out: &mut Vec<Link>) -> Res<()> {
        if self.rd(at, 4)? != b"SNOD" {
            return Err("bad symbol table node signature".into());
        }
        let n = self.uint(at + 6, 2)? as usize;
        if out.len().saturating_add(n) > MAX_OBJECTS {
            return Err("group has too many members to list".into());
        }
        let esz = 2 * self.so + 8 + 16;
        for i in 0..n {
            let e = at + 8 + i * esz;
            let name_off = self.off(e)? as usize;
            let obj = self.off(e + self.so)?;
            let cache = self.uint(e + 2 * self.so, 4)?;
            let name_bytes = heap.get(name_off..).ok_or("symbol name offset is outside its heap")?;
            let nb = &name_bytes[..name_bytes.len().min(MAX_NAME)];
            if !nb.contains(&0) && name_bytes.len() > MAX_NAME {
                return Err("symbol name is implausibly long".into());
            }
            let name = cstr(nb);
            let target = if cache == 2 {
                // soft link: scratch holds the offset of the target in the heap
                let so_off = self.uint(e + 2 * self.so + 8, 4)? as usize;
                let tb = heap.get(so_off..).ok_or("soft link target is outside its heap")?;
                LinkTarget::Soft(cstr(&tb[..tb.len().min(MAX_NAME)]))
            } else {
                LinkTarget::Hard(self.need_addr_raw(obj, "symbol table entry")?)
            };
            out.push(Link { name, target });
        }
        Ok(())
    }

    // -------------------------------------------------------- datasets

    fn dataspace(&self, d: &[u8]) -> Res<Vec<usize>> {
        let ver = le(d, 0, 1)?;
        let rank = le(d, 1, 1)? as usize;
        let (dims_at, null) = match ver {
            1 => (8, false),
            2 => (4, le(d, 3, 1)? == 2),
            v => return Err(format!("unsupported dataspace version {v}")),
        };
        if null {
            return Err("null dataspace (a dataset with no data)".into());
        }
        if rank > MAX_RANK {
            return Err(format!("dataspace rank {rank} is implausible"));
        }
        let mut shape = Vec::with_capacity(rank);
        for i in 0..rank {
            let v = le(d, dims_at + i * self.sl, self.sl)?;
            shape.push(usize::try_from(v).map_err(|_| "dimension does not fit in memory")?);
        }
        Ok(shape)
    }

    fn datatype(&self, d: &[u8]) -> Res<H5Type> {
        let b0 = le(d, 0, 1)?;
        let class = b0 & 0x0f;
        let bits = le(d, 1, 3)?;
        let size = le(d, 4, 4)? as usize;
        Ok(match class {
            0 => {
                let off = le(d, 8, 2)?;
                let prec = le(d, 10, 2)? as usize;
                if off != 0 || prec != size * 8 {
                    return Ok(H5Type::Unsupported(format!("packed {prec}-bit integer")));
                }
                if !matches!(size, 1 | 2 | 4 | 8) {
                    return Ok(H5Type::Unsupported(format!("{size}-byte integer")));
                }
                H5Type::Fixed { size, signed: bits & 0x08 != 0, big: bits & 1 != 0 }
            }
            1 => {
                let order = ((bits >> 5) & 2) | (bits & 1);
                if order > 1 {
                    return Ok(H5Type::Unsupported("VAX-order float".into()));
                }
                let sign_loc = (bits >> 8) & 0xff;
                let (eloc, esz, mloc, msz) = (le(d, 12, 1)?, le(d, 13, 1)?, le(d, 14, 1)?, le(d, 15, 1)?);
                let bias = le(d, 16, 4)?;
                let off = le(d, 8, 2)?;
                let ieee = match size {
                    2 => (eloc, esz, mloc, msz, bias) == (10, 5, 0, 10, 15),
                    4 => (eloc, esz, mloc, msz, bias) == (23, 8, 0, 23, 127),
                    8 => (eloc, esz, mloc, msz, bias) == (52, 11, 0, 52, 1023),
                    _ => false,
                };
                if !ieee || off != 0 || sign_loc as usize != size * 8 - 1 {
                    return Ok(H5Type::Unsupported(format!("non-IEEE {}-byte float layout", size)));
                }
                H5Type::Float { size, big: order == 1 }
            }
            3 => H5Type::FixedStr { size, utf8: (bits >> 4) & 0x0f == 1 },
            4 => H5Type::Unsupported("bitfield".into()),
            5 => H5Type::Unsupported("opaque".into()),
            6 => H5Type::Unsupported("compound".into()),
            7 => H5Type::Unsupported("reference".into()),
            8 => {
                let base = self.datatype(d.get(8..).ok_or("truncated enum datatype")?)?;
                match base {
                    H5Type::Fixed { .. } => H5Type::Enum(Box::new(base)),
                    _ => H5Type::Unsupported("enum with a non-integer base".into()),
                }
            }
            9 => {
                if bits & 0x0f == 1 {
                    H5Type::VlenStr
                } else {
                    H5Type::Unsupported("variable-length sequence".into())
                }
            }
            10 => H5Type::Unsupported("array".into()),
            c => H5Type::Unsupported(format!("datatype class {c}")),
        })
    }

    fn layout(&self, d: &[u8]) -> Res<Layout> {
        let ver = le(d, 0, 1)?;
        if ver != 3 && ver != 4 {
            return Err(format!(
                "data layout message version {ver} is not supported (only versions 3 and 4, written by HDF5 1.8 and later)"
            ));
        }
        let class = le(d, 1, 1)?;
        match class {
            0 => {
                let sz = le(d, 2, 2)? as usize;
                Ok(Layout::Compact(d.get(4..4 + sz).ok_or("truncated compact dataset")?.to_vec()))
            }
            1 => {
                let addr = self.addr(le(d, 2, self.so)?)?;
                let size = usize::try_from(le(d, 2 + self.so, self.sl)?).map_err(|_| "dataset size does not fit in memory")?;
                Ok(Layout::Contiguous { addr, size })
            }
            2 if ver == 3 => {
                let dimensionality = le(d, 2, 1)? as usize;
                if !(2..=MAX_RANK + 1).contains(&dimensionality) {
                    return Err(format!("chunk dimensionality {dimensionality} is implausible"));
                }
                let bt = self.addr(le(d, 3, self.so)?)?;
                let mut chunk = Vec::new();
                for i in 0..dimensionality {
                    chunk.push(le(d, 3 + self.so + 4 * i, 4)? as usize);
                }
                // `chunk` ends with the element size; the reader splits it off.
                // An undefined B-tree address means no chunk was ever written.
                Ok(Layout::Chunked { btree: bt.unwrap_or(usize::MAX), chunk })
            }
            2 => {
                // version 4 chunked
                let flags = le(d, 2, 1)?;
                let dimensionality = le(d, 3, 1)? as usize;
                let enc = le(d, 4, 1)? as usize;
                if !(2..=MAX_RANK + 1).contains(&dimensionality) || !(1..=8).contains(&enc) {
                    return Err("corrupt chunked layout message".into());
                }
                let mut p = 5;
                let mut chunk = Vec::new();
                for _ in 0..dimensionality {
                    chunk.push(le(d, p, enc)? as usize);
                    p += enc;
                }
                let idx = le(d, p, 1)?;
                p += 1;
                if idx != 1 {
                    let name = match idx {
                        2 => "implicit",
                        3 => "fixed array",
                        4 => "extensible array",
                        5 => "version 2 B-tree",
                        _ => "unknown",
                    };
                    return Err(format!(
                        "chunked dataset uses a {name} chunk index (libver='latest'); only the single-chunk index is supported"
                    ));
                }
                let (filtered_size, mask) = if flags & 0x02 != 0 {
                    let s = le(d, p, self.sl)? as usize;
                    p += self.sl;
                    let m = le(d, p, 4)? as u32;
                    p += 4;
                    (Some(s), m)
                } else {
                    (None, 0)
                };
                let addr = self.addr(le(d, p, self.so)?)?;
                Ok(Layout::SingleChunk { addr, chunk, filtered_size, mask })
            }
            c => Err(format!("unknown data layout class {c}")),
        }
    }

    fn filters(&self, d: &[u8]) -> Res<Vec<Filter>> {
        let ver = le(d, 0, 1)?;
        let n = le(d, 1, 1)? as usize;
        let mut p = if ver == 1 { 8 } else { 2 };
        if ver != 1 && ver != 2 {
            return Err(format!("unsupported filter pipeline version {ver}"));
        }
        let mut out = Vec::new();
        for _ in 0..n {
            let id = le(d, p, 2)? as u16;
            p += 2;
            let name_len = if ver == 1 || id >= 256 {
                let l = le(d, p, 2)? as usize;
                p += 2;
                l
            } else {
                0
            };
            let _flags = le(d, p, 2)?;
            let ncd = le(d, p + 2, 2)? as usize;
            p += 4;
            if ver == 1 {
                p += (name_len + 7) / 8 * 8;
            } else {
                p += name_len;
            }
            let mut cd = Vec::with_capacity(ncd.min(64));
            for i in 0..ncd {
                cd.push(le(d, p + 4 * i, 4)? as u32);
            }
            p += 4 * ncd;
            if ver == 1 && ncd % 2 == 1 {
                p += 4;
            }
            out.push(Filter { id, cd });
        }
        Ok(out)
    }

    fn fill_value(&self, m: &Msg<'a>) -> Res<Option<Vec<u8>>> {
        let d = m.data;
        if m.ty == 0x04 {
            let sz = le(d, 0, 4)? as usize;
            if sz == 0 {
                return Ok(None);
            }
            return Ok(Some(d.get(4..4 + sz).ok_or("truncated fill value")?.to_vec()));
        }
        let ver = le(d, 0, 1)?;
        let (defined, p) = match ver {
            1 | 2 => (le(d, 3, 1)? == 1, 4),
            3 => (le(d, 1, 1)? & 0x20 != 0, 2),
            v => return Err(format!("unsupported fill value version {v}")),
        };
        if !defined {
            return Ok(None);
        }
        let sz = le(d, p, 4)? as usize;
        if sz == 0 {
            return Ok(None);
        }
        Ok(Some(d.get(p + 4..p + 4 + sz).ok_or("truncated fill value")?.to_vec()))
    }

    fn dataset_meta(&self, msgs: &[Msg<'a>]) -> Res<Dataset> {
        let mut shape = None;
        let mut ty = None;
        let mut layout = None;
        let mut filters = Vec::new();
        let mut fill = None;
        for m in msgs {
            match m.ty {
                0x01 => shape = Some(self.dataspace(m.data)?),
                0x03 => ty = Some(self.datatype(m.data)?),
                0x08 => layout = Some(self.layout(m.data)?),
                0x0b => filters = self.filters(m.data)?,
                0x04 | 0x05 => fill = self.fill_value(m)?,
                _ => {}
            }
        }
        Ok(Dataset {
            shape: shape.ok_or("object has no dataspace message (not a dataset)")?,
            ty: ty.ok_or("object has no datatype message (not a dataset)")?,
            layout: layout.ok_or("object has no data layout message (not a dataset)")?,
            filters,
            fill,
        })
    }

    // ------------------------------------------------------- reading data

    /// Raw bytes of the dataset in C order, `nelem * elem` long.
    fn raw_data(&self, ds: &Dataset, elem: usize) -> Res<Vec<u8>> {
        let nelem = checked_product(&ds.shape)?;
        // The decoded form is one f64 per element, so bound that as well as
        // the raw bytes: a million-element-per-byte shape must not turn a
        // 1 GiB raw read into an 8 GiB vector.
        nelem
            .checked_mul(elem.max(8))
            .filter(|&t| t <= MAX_DATA_BYTES)
            .ok_or_else(|| {
                format!(
                    "dataset of shape {:?} exceeds the {} MiB limit",
                    ds.shape,
                    MAX_DATA_BYTES >> 20
                )
            })?;
        let total = nelem
            .checked_mul(elem)
            .filter(|&t| t <= MAX_DATA_BYTES)
            .ok_or_else(|| {
                format!(
                    "dataset of shape {:?} ({elem}-byte elements) exceeds the {} MiB limit",
                    ds.shape,
                    MAX_DATA_BYTES >> 20
                )
            })?;
        if !ds.filters.is_empty() && !matches!(ds.layout, Layout::Chunked { .. } | Layout::SingleChunk { .. }) {
            return Err("a filter pipeline on non-chunked storage is invalid".into());
        }
        let fill_buf = |total: usize| -> Vec<u8> {
            match &ds.fill {
                Some(f) if f.len() == elem && f.iter().any(|&b| b != 0) => {
                    let mut v = Vec::with_capacity(total);
                    while v.len() < total {
                        v.extend_from_slice(f);
                    }
                    v
                }
                _ => vec![0u8; total],
            }
        };
        match &ds.layout {
            Layout::Compact(data) => {
                if data.len() < total {
                    return Err("compact dataset holds fewer bytes than its shape needs".into());
                }
                Ok(data[..total].to_vec())
            }
            Layout::Contiguous { addr, size } => match addr {
                None => Ok(fill_buf(total)),
                Some(a) => {
                    if *size < total {
                        return Err(format!(
                            "contiguous storage holds {size} bytes but the shape needs {total}"
                        ));
                    }
                    Ok(self.rd(*a, total)?.to_vec())
                }
            },
            Layout::SingleChunk { addr, chunk, filtered_size, mask } => {
                let rank = ds.shape.len();
                if chunk.len() != rank + 1 && chunk.len() != rank {
                    return Err("chunk rank does not match the dataspace".into());
                }
                let mut out = fill_buf(total);
                if let Some(a) = addr {
                    let n = filtered_size.unwrap_or(total.min(MAX_DATA_BYTES));
                    let raw = self.rd(*a, n)?;
                    let cdims: Vec<usize> = chunk[..rank].to_vec();
                    let offs = vec![0usize; rank];
                    let data = self.unfilter(raw, &ds.filters, *mask, checked_product(&cdims)?.saturating_mul(elem), elem)?;
                    place_chunk(&mut out, &ds.shape, &cdims, &offs, &data, elem)?;
                }
                Ok(out)
            }
            Layout::Chunked { btree, chunk } => {
                let rank = ds.shape.len();
                if chunk.len() != rank + 1 {
                    return Err("chunk rank does not match the dataspace".into());
                }
                if chunk[rank] != elem {
                    return Err(format!(
                        "chunk element size {} disagrees with the datatype size {elem}",
                        chunk[rank]
                    ));
                }
                let cdims: Vec<usize> = chunk[..rank].to_vec();
                if cdims.iter().any(|&c| c == 0) {
                    return Err("chunk dimension of zero".into());
                }
                let cbytes = checked_product(&cdims)?
                    .checked_mul(elem)
                    .filter(|&c| c <= MAX_DATA_BYTES)
                    .ok_or("chunk size is implausibly large")?;
                let mut out = fill_buf(total);
                if *btree == usize::MAX {
                    return Ok(out);
                }
                let mut chunks = Vec::new();
                self.chunk_btree(*btree, rank, &mut chunks)?;
                let mut budget = 0usize;
                for (offs, size, mask, addr) in chunks {
                    // Total decoded bytes over all chunks: entries may share
                    // one address, so count every placement, not every file byte.
                    budget = budget.saturating_add(cbytes);
                    if budget > 2 * MAX_DATA_BYTES {
                        return Err("chunk index asks for more decompressed data than the limit allows (repeated or overlapping chunks)".into());
                    }
                    let raw = self.rd(addr, size)?;
                    let data = self.unfilter(raw, &ds.filters, mask, cbytes, elem)?;
                    place_chunk(&mut out, &ds.shape, &cdims, &offs, &data, elem)?;
                }
                Ok(out)
            }
        }
    }

    /// Walk a v1 chunk B-tree, collecting (offsets, stored size, filter
    /// mask, file position) per chunk.
    fn chunk_btree(
        &self,
        root: usize,
        rank: usize,
        out: &mut Vec<(Vec<usize>, usize, u32, usize)>,
    ) -> Res<()> {
        let key_size = 8 + 8 * (rank + 1);
        let mut stack = vec![(root, 0usize)];
        let mut nodes = 0usize;
        while let Some((node, depth)) = stack.pop() {
            nodes += 1;
            if nodes > MAX_NODES || depth > MAX_DEPTH {
                return Err("chunk B-tree is too deep or cyclic".into());
            }
            if self.rd(node, 4)? != b"TREE" {
                return Err("bad chunk B-tree node signature".into());
            }
            if self.uint(node + 4, 1)? != 1 {
                return Err("chunk B-tree node has the wrong node type".into());
            }
            let level = self.uint(node + 5, 1)?;
            let used = self.uint(node + 6, 2)? as usize;
            let mut p = node + 8 + 2 * self.so;
            for _ in 0..used {
                let size = self.uint(p, 4)? as usize;
                let mask = self.uint(p + 4, 4)? as u32;
                let mut offs = Vec::with_capacity(rank);
                for i in 0..rank {
                    offs.push(usize::try_from(self.uint(p + 8 + 8 * i, 8)?).map_err(|_| "chunk offset too large")?);
                }
                let child = self.need_addr(p + key_size, "chunk B-tree child")?;
                p += key_size + self.so;
                if level > 0 {
                    stack.push((child, depth + 1));
                } else {
                    if out.len() >= MAX_NODES {
                        return Err("dataset has too many chunks".into());
                    }
                    out.push((offs, size, mask, child));
                }
            }
        }
        Ok(())
    }

    fn unfilter(&self, raw: &[u8], filters: &[Filter], mask: u32, expect: usize, _elem: usize) -> Res<Vec<u8>> {
        let mut data = raw.to_vec();
        for (i, f) in filters.iter().enumerate().rev() {
            if i < 32 && mask & (1 << i) != 0 {
                continue; // this filter was skipped for this chunk
            }
            data = match f.id {
                1 => zlib_decompress_capped(&data, expect.max(1))
                    .map_err(|e| format!("deflate filter: {e}"))?,
                2 => {
                    let e = *f.cd.first().ok_or("shuffle filter has no element size")? as usize;
                    unshuffle(&data, e)
                }
                3 => {
                    if data.len() < 4 {
                        return Err("fletcher32 filter: chunk shorter than its checksum".into());
                    }
                    data[..data.len() - 4].to_vec()
                }
                id => {
                    let name = match id {
                        4 => "szip",
                        5 => "n-bit",
                        6 => "scale-offset",
                        307 => "bzip2",
                        32000 => "LZF",
                        32001 => "Blosc",
                        32004 => "LZ4",
                        32015 => "Zstandard",
                        _ => "unknown",
                    };
                    return Err(format!(
                        "dataset uses the {name} filter (id {id}), which is not supported; only deflate, shuffle and fletcher32 are"
                    ));
                }
            };
        }
        if data.len() < expect {
            return Err(format!(
                "chunk decodes to {} bytes but {expect} were expected (corrupt or truncated)",
                data.len()
            ));
        }
        Ok(data)
    }

    fn read_dataset(&self, msgs: &[Msg<'a>]) -> Res<H5Value> {
        let ds = self.dataset_meta(msgs)?;
        if let H5Type::Unsupported(s) = &ds.ty {
            return Err(format!("dataset datatype `{s}` is not supported"));
        }
        let elem = ds.ty.size(self.so).ok_or("datatype has no size")?;
        if elem == 0 {
            return Err("zero-size datatype".into());
        }
        let raw = self.raw_data(&ds, elem)?;
        let n = checked_product(&ds.shape)?;
        match &ds.ty {
            H5Type::VlenStr => {
                let mut heaps: HashMap<usize, HashMap<u16, (usize, usize)>> = HashMap::new();
                let mut total_text = 0usize;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let e = &raw[i * elem..(i + 1) * elem];
                    let len = le(e, 0, 4)? as usize;
                    let ga = le(e, 4, self.so)?;
                    let idx = le(e, 4 + self.so, 4)? as u16;
                    if len == 0 {
                        out.push(String::new());
                        continue;
                    }
                    let Some(gpos) = self.addr(ga)? else {
                        out.push(String::new());
                        continue;
                    };
                    if !heaps.contains_key(&gpos) {
                        let objs = self.global_heap(gpos)?;
                        heaps.insert(gpos, objs);
                    }
                    let &(start, size) = heaps[&gpos]
                        .get(&idx)
                        .ok_or("variable-length string refers to a missing global heap object")?;
                    let size = size.min(len);
                    total_text = total_text.saturating_add(size);
                    if total_text > MAX_TEXT_BYTES {
                        return Err(format!(
                            "variable-length strings total more than the {} MiB limit",
                            MAX_TEXT_BYTES >> 20
                        ));
                    }
                    out.push(String::from_utf8_lossy(&self.b[start..start + size]).into_owned());
                }
                Ok(H5Value::Text { shape: ds.shape, data: out })
            }
            H5Type::FixedStr { size, .. } => {
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    out.push(cstr(&raw[i * size..(i + 1) * size]).trim_end_matches(' ').to_string());
                }
                Ok(H5Value::Text { shape: ds.shape, data: out })
            }
            ty => {
                let base = if let H5Type::Enum(b) = ty { b.as_ref() } else { ty };
                let mut out = Vec::with_capacity(n);
                for c in raw.chunks_exact(elem).take(n) {
                    out.push(decode_scalar(base, c)?);
                }
                Ok(H5Value::Real { shape: ds.shape, data: out })
            }
        }
    }

    /// Index -> (data position, size) for every object in a global heap collection.
    fn global_heap(&self, at: usize) -> Res<HashMap<u16, (usize, usize)>> {
        if self.rd(at, 4)? != b"GCOL" {
            return Err("bad global heap signature".into());
        }
        let csize = self.len_at(at + 8)? as usize;
        let end = at.checked_add(csize).filter(|&e| e <= self.b.len()).ok_or("global heap runs past the end of the file")?;
        let mut p = at + 8 + self.sl;
        let mut out = HashMap::new();
        let mut guard = 0usize;
        while p + 8 + self.sl <= end {
            guard += 1;
            if guard > MAX_NODES {
                return Err("global heap has too many objects".into());
            }
            let idx = self.uint(p, 2)? as u16;
            if idx == 0 {
                break;
            }
            let size = self.len_at(p + 8)? as usize;
            let dstart = p + 8 + self.sl;
            dstart.checked_add(size).filter(|&e| e <= end).ok_or("global heap object runs past its collection")?;
            out.insert(idx, (dstart, size));
            p = dstart + (size + 7) / 8 * 8;
        }
        Ok(out)
    }

    // --------------------------------------------------------- attributes

    /// Decode a fixed-length-string attribute (all that MAT v7.3 needs).
    fn attr_string(&self, msgs: &[Msg<'a>], name: &str) -> Option<String> {
        for m in msgs.iter().filter(|m| m.ty == 0x0c) {
            if let Ok(Some((n, ty, shape, data))) = self.attribute(m.data) {
                if n == name && shape.is_empty() {
                    if let H5Type::FixedStr { size, .. } = ty {
                        return data.get(..size).map(|s| cstr(s));
                    }
                }
            }
        }
        None
    }

    fn attr_u64(&self, msgs: &[Msg<'a>], name: &str) -> Option<u64> {
        for m in msgs.iter().filter(|m| m.ty == 0x0c) {
            if let Ok(Some((n, ty, _shape, data))) = self.attribute(m.data) {
                if n == name {
                    if let H5Type::Fixed { size, .. } = ty {
                        return le(&data, 0, size).ok();
                    }
                }
            }
        }
        None
    }

    #[allow(clippy::type_complexity)]
    fn attribute(&self, d: &[u8]) -> Res<Option<(String, H5Type, Vec<usize>, Vec<u8>)>> {
        let ver = le(d, 0, 1)?;
        if !(1..=3).contains(&ver) {
            return Ok(None);
        }
        let nsz = le(d, 2, 2)? as usize;
        let tsz = le(d, 4, 2)? as usize;
        let ssz = le(d, 6, 2)? as usize;
        let mut p = if ver == 3 { 9 } else { 8 };
        let pad = |n: usize| if ver == 1 { (n + 7) / 8 * 8 } else { n };
        let name = cstr(d.get(p..p + nsz).ok_or("truncated attribute")?);
        p += pad(nsz);
        let ty = self.datatype(d.get(p..p + tsz).ok_or("truncated attribute")?)?;
        p += pad(tsz);
        let shape = self.dataspace(d.get(p..p + ssz).ok_or("truncated attribute")?)?;
        p += pad(ssz);
        let data = d.get(p..).ok_or("truncated attribute")?.to_vec();
        Ok(Some((name, ty, shape, data)))
    }

    // ------------------------------------------------------- navigation

    fn resolve(&self, path: &str) -> Res<usize> {
        self.resolve_from(self.root, path, 0)
    }

    fn resolve_from(&self, start: usize, path: &str, depth: usize) -> Res<usize> {
        if depth > 16 {
            return Err("soft links nest too deeply (a link cycle?)".into());
        }
        let mut cur = if path.starts_with('/') { self.root } else { start };
        for part in path.split('/').filter(|p| !p.is_empty() && *p != ".") {
            let msgs = self.header(cur)?;
            let links = self
                .links(&msgs)?
                .ok_or_else(|| format!("`{part}`: the parent in `{path}` is not a group"))?;
            let link = links
                .into_iter()
                .find(|l| l.name == part)
                .ok_or_else(|| format!("`{path}` not found (no member named `{part}`)"))?;
            cur = match link.target {
                LinkTarget::Hard(a) => a,
                LinkTarget::Soft(t) => self.resolve_from(cur, &t, depth + 1)?,
                LinkTarget::External => {
                    return Err(format!("`{part}` is an external link to another file, which is not supported"))
                }
            };
        }
        Ok(cur)
    }

    fn describe(&self, at: usize) -> Res<(&'static str, String, Vec<usize>)> {
        let msgs = self.header(at)?;
        if self.links(&msgs)?.is_some() {
            return Ok(("group", String::new(), vec![]));
        }
        if msgs.iter().any(|m| m.ty == 0x08) {
            let shape = msgs
                .iter()
                .find(|m| m.ty == 0x01)
                .map(|m| self.dataspace(m.data))
                .transpose()
                .unwrap_or(None)
                .unwrap_or_default();
            let dtype = msgs
                .iter()
                .find(|m| m.ty == 0x03)
                .map(|m| self.datatype(m.data).map(|t| t.describe()).unwrap_or_else(|e| format!("unsupported({e})")))
                .unwrap_or_default();
            return Ok(("dataset", dtype, shape));
        }
        if msgs.iter().any(|m| m.ty == 0x03) {
            return Ok(("datatype", String::new(), vec![]));
        }
        Ok(("group", String::new(), vec![]))
    }

    fn walk(
        &self,
        at: usize,
        path: &str,
        out: &mut Vec<H5Entry>,
        seen: &mut HashSet<usize>,
        depth: usize,
    ) -> Res<()> {
        if depth > MAX_DEPTH {
            return Err("group nesting is too deep".into());
        }
        let msgs = self.header(at)?;
        let Some(mut links) = self.links(&msgs)? else { return Ok(()) };
        links.sort_by(|a, b| a.name.cmp(&b.name));
        for l in links {
            if out.len() >= MAX_OBJECTS {
                return Err("file has too many objects to list".into());
            }
            let p = format!("{}/{}", path.trim_end_matches('/'), l.name);
            match l.target {
                LinkTarget::Hard(a) => {
                    let (kind, dtype, shape) = self.describe(a)?;
                    out.push(H5Entry { path: p.clone(), kind, dtype, shape });
                    if kind == "group" && seen.insert(a) {
                        self.walk(a, &p, out, seen, depth + 1)?;
                    }
                }
                LinkTarget::Soft(t) => out.push(H5Entry { path: p, kind: "softlink", dtype: format!("-> {t}"), shape: vec![] }),
                LinkTarget::External => out.push(H5Entry { path: p, kind: "externallink", dtype: String::new(), shape: vec![] }),
            }
        }
        Ok(())
    }
}

fn check_widths(so: usize, sl: usize) -> Res<()> {
    if matches!(so, 2 | 4 | 8) && matches!(sl, 2 | 4 | 8) {
        Ok(())
    } else {
        Err(format!("unsupported HDF5 offset/length sizes ({so}/{sl})"))
    }
}

fn unshuffle(data: &[u8], elem: usize) -> Vec<u8> {
    if elem <= 1 {
        return data.to_vec();
    }
    let n = data.len() / elem;
    let mut out = vec![0u8; data.len()];
    for i in 0..n {
        for j in 0..elem {
            out[i * elem + j] = data[j * n + i];
        }
    }
    // trailing bytes that did not fill an element are stored verbatim
    out[n * elem..].copy_from_slice(&data[n * elem..]);
    out
}

/// Copy one decoded chunk (row-major, `cdims`) into the dataset buffer at
/// element offset `offs`, clipping at the dataset edge.
fn place_chunk(
    out: &mut [u8],
    shape: &[usize],
    cdims: &[usize],
    offs: &[usize],
    chunk: &[u8],
    elem: usize,
) -> Res<()> {
    let rank = shape.len();
    if rank == 0 {
        let n = elem.min(chunk.len()).min(out.len());
        out[..n].copy_from_slice(&chunk[..n]);
        return Ok(());
    }
    // dataset strides in elements
    let mut dstride = vec![1usize; rank];
    for i in (0..rank - 1).rev() {
        dstride[i] = dstride[i + 1] * shape[i + 1];
    }
    let mut cstride = vec![1usize; rank];
    for i in (0..rank - 1).rev() {
        cstride[i] = cstride[i + 1] * cdims[i + 1];
    }
    if offs.iter().zip(shape).any(|(&o, &s)| o >= s) {
        return Ok(()); // chunk lies entirely outside the dataspace
    }
    let run = cdims[rank - 1].min(shape[rank - 1] - offs[rank - 1]);
    let outer: usize = cdims[..rank - 1].iter().product();
    let mut idx = vec![0usize; rank - 1];
    for _ in 0..outer {
        let mut inside = true;
        let mut dpos = offs[rank - 1];
        let mut cpos = 0usize;
        for a in 0..rank - 1 {
            let g = offs[a] + idx[a];
            if g >= shape[a] {
                inside = false;
                break;
            }
            dpos += g * dstride[a];
            cpos += idx[a] * cstride[a];
        }
        if inside {
            let (d0, c0) = (dpos * elem, cpos * elem);
            let nb = run * elem;
            if d0 + nb > out.len() || c0 + nb > chunk.len() {
                return Err("chunk placement out of range (corrupt chunk index)".into());
            }
            out[d0..d0 + nb].copy_from_slice(&chunk[c0..c0 + nb]);
        }
        for a in (0..rank - 1).rev() {
            idx[a] += 1;
            if idx[a] < cdims[a] {
                break;
            }
            idx[a] = 0;
        }
    }
    Ok(())
}

fn decode_scalar(ty: &H5Type, c: &[u8]) -> Res<f64> {
    let mut buf = [0u8; 8];
    match ty {
        H5Type::Fixed { size, signed, big } => {
            buf[..*size].copy_from_slice(&c[..*size]);
            if *big {
                buf[..*size].reverse();
            }
            Ok(match (*size, *signed) {
                (1, true) => buf[0] as i8 as f64,
                (1, false) => buf[0] as f64,
                (2, true) => i16::from_le_bytes([buf[0], buf[1]]) as f64,
                (2, false) => u16::from_le_bytes([buf[0], buf[1]]) as f64,
                (4, true) => i32::from_le_bytes(buf[..4].try_into().unwrap()) as f64,
                (4, false) => u32::from_le_bytes(buf[..4].try_into().unwrap()) as f64,
                (8, true) => {
                    let v = i64::from_le_bytes(buf);
                    if v.unsigned_abs() > (1u64 << 53) {
                        return Err(format!("int64 value {v} cannot be held exactly in a 64-bit float"));
                    }
                    v as f64
                }
                (8, false) => {
                    let v = u64::from_le_bytes(buf);
                    if v > (1u64 << 53) {
                        return Err(format!("uint64 value {v} cannot be held exactly in a 64-bit float"));
                    }
                    v as f64
                }
                _ => return Err("unsupported integer size".into()),
            })
        }
        H5Type::Float { size, big } => {
            buf[..*size].copy_from_slice(&c[..*size]);
            if *big {
                buf[..*size].reverse();
            }
            Ok(match size {
                2 => {
                    let h = u16::from_le_bytes([buf[0], buf[1]]);
                    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
                    let exp = ((h >> 10) & 0x1f) as i32;
                    let frac = (h & 0x3ff) as f64;
                    match exp {
                        0 => sign * frac * 2f64.powi(-24),
                        31 => if frac == 0.0 { sign * f64::INFINITY } else { f64::NAN },
                        _ => sign * (1.0 + frac / 1024.0) * 2f64.powi(exp - 15),
                    }
                }
                4 => f32::from_le_bytes(buf[..4].try_into().unwrap()) as f64,
                8 => f64::from_le_bytes(buf),
                _ => return Err("unsupported float size".into()),
            })
        }
        other => Err(format!("cannot read datatype {}", other.describe())),
    }
}

// ------------------------------------------------------------- public API

/// List every group, dataset and link in the file, depth-first, sorted by
/// name within each group.
pub fn h5info(bytes: &[u8]) -> Res<Vec<H5Entry>> {
    let f = Hdf5::open(bytes)?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    seen.insert(f.root);
    f.walk(f.root, "/", &mut out, &mut seen, 0)?;
    Ok(out)
}

/// Read one dataset by path (`/group/name` or `group/name`).
pub fn h5read(bytes: &[u8], dataset: &str) -> Res<H5Value> {
    let f = Hdf5::open(bytes)?;
    let at = f.resolve(dataset)?;
    let msgs = f.header(at)?;
    if f.links(&msgs)?.is_some() {
        return Err(format!("`{dataset}` is a group, not a dataset (see h5info)"));
    }
    f.read_dataset(&msgs).map_err(|e| format!("`{dataset}`: {e}"))
}

// ---------------------------------------------------- MATLAB v7.3 support

/// Is this a MATLAB v7.3 `.mat` file (an HDF5 container behind a 512-byte
/// text header)?
pub fn is_mat_v73(bytes: &[u8]) -> bool {
    bytes.len() > 520 && bytes.starts_with(b"MATLAB 7.3") && &bytes[512..520] == SIGNATURE
}

/// Read the variables of a MATLAB v7.3 file: numeric/logical arrays, char
/// row vectors and scalar structs. Cell arrays, complex values, sparse
/// matrices, N-d arrays and anything else read as `Unsupported`, so one
/// exotic variable cannot make the whole file unreadable.
pub fn read_mat_v73(bytes: &[u8]) -> Res<Vec<(String, MatValue)>> {
    let f = Hdf5::open(bytes)?;
    let msgs = f.header(f.root)?;
    let links = f.links(&msgs)?.ok_or("root of the v7.3 file is not a group")?;
    let mut out = Vec::new();
    for l in links {
        if l.name.starts_with('#') {
            continue; // `#refs#`, `#subsystem#`: bookkeeping
        }
        if let LinkTarget::Hard(a) = l.target {
            out.push((l.name, f.mat_object(a, 0)));
        }
    }
    Ok(out)
}

impl<'a> Hdf5<'a> {
    fn mat_object(&self, at: usize, depth: usize) -> MatValue {
        const BAD: MatValue = MatValue::Unsupported("v7.3 variable (try h5read)");
        let Ok(msgs) = self.header(at) else { return BAD };
        let class = self.attr_string(&msgs, "MATLAB_class").unwrap_or_default();
        if let Ok(Some(links)) = self.links(&msgs) {
            if self.attr_u64(&msgs, "MATLAB_sparse").is_some() {
                return MatValue::Unsupported("sparse matrix (v7.3)");
            }
            if depth > 16 {
                return BAD;
            }
            let mut fields = Vec::new();
            for l in links {
                if let LinkTarget::Hard(a) = l.target {
                    fields.push((l.name, self.mat_object(a, depth + 1)));
                }
            }
            return MatValue::Struct(fields);
        }
        if self.attr_u64(&msgs, "MATLAB_empty").is_some() {
            return match self.read_dataset(&msgs) {
                Ok(H5Value::Real { data, .. }) => {
                    MatValue::Numeric { dims: data.iter().map(|&d| d as usize).collect(), data: vec![] }
                }
                _ => BAD,
            };
        }
        let Ok(v) = self.read_dataset(&msgs) else {
            return if class == "cell" { MatValue::Unsupported("cell array (v7.3)") } else { BAD };
        };
        let H5Value::Real { shape, data } = v else { return BAD };
        // HDF5 stores MATLAB's column-major array as the row-major array of
        // the reversed dimensions, so the bytes are already in MATLAB order.
        let mut dims: Vec<usize> = shape.iter().rev().copied().collect();
        if dims.len() == 1 {
            dims.push(1);
        }
        if dims.len() > 2 {
            return MatValue::Unsupported("N-d array (v7.3)");
        }
        if class == "char" {
            if dims[0] <= 1 {
                return MatValue::Str(data.iter().filter_map(|&c| char::from_u32(c as u32)).collect());
            }
            return MatValue::Unsupported("multi-row char array (v7.3)");
        }
        MatValue::Numeric { dims, data }
    }
}
