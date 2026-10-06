//! NumPy (`.npy`/`.npz`) and HDF5 (`h5read`/`h5info`, MATLAB v7.3) interop.
//!
//! No fixture came from NumPy or h5py (neither can be assumed on a build
//! machine): the `.npy` bytes follow the NumPy format spec and the HDF5
//! files are built byte by byte by the small writer below, from the HDF5
//! File Format Specification. That proves the reader agrees with the spec
//! as the author read it, not with libhdf5 -- the report says so.

use qu_interp::hdf5::{self, H5Value};
use qu_interp::npy::{self, NpyArray, NpyData};
use qu_interp::Interp;

// ================================================================== NPY

/// Build a `.npy` file the way NumPy's writer lays it out.
fn npy_file(major: u8, descr: &str, fortran: bool, shape: &str, payload: &[u8]) -> Vec<u8> {
    let mut h = format!(
        "{{'descr': '{descr}', 'fortran_order': {}, 'shape': {shape}, }}",
        if fortran { "True" } else { "False" }
    );
    let prefix = if major == 1 { 10 } else { 12 };
    let total = prefix + h.len() + 1;
    h.push_str(&" ".repeat((64 - total % 64) % 64));
    h.push('\n');
    let mut out = b"\x93NUMPY".to_vec();
    out.push(major);
    out.push(0);
    if major == 1 {
        out.extend_from_slice(&(h.len() as u16).to_le_bytes());
    } else {
        out.extend_from_slice(&(h.len() as u32).to_le_bytes());
    }
    out.extend_from_slice(h.as_bytes());
    out.extend_from_slice(payload);
    out
}

fn real(a: &NpyArray) -> &Vec<f64> {
    match &a.data {
        NpyData::Real(v) => v,
        other => panic!("expected real data, got {other:?}"),
    }
}

#[test]
fn npy_reads_every_float_and_int_dtype() {
    let le = |v: &[f64], f: &dyn Fn(f64) -> Vec<u8>| -> Vec<u8> { v.iter().flat_map(|&x| f(x)).collect() };
    // float16: 1.0 = 0x3C00, -2.0 = 0xC000, 0.5 = 0x3800, inf = 0x7C00
    let f2: Vec<u8> = [0x3C00u16, 0xC000, 0x3800, 0x7C00].iter().flat_map(|h| h.to_le_bytes()).collect();
    let a = npy::read_npy(&npy_file(1, "<f2", false, "(4,)", &f2)).unwrap();
    assert_eq!(real(&a), &vec![1.0, -2.0, 0.5, f64::INFINITY]);
    assert_eq!(a.shape, vec![4]);

    let a = npy::read_npy(&npy_file(1, "<f4", false, "(3,)", &le(&[1.5, -0.25, 1e10], &|x| (x as f32).to_le_bytes().to_vec()))).unwrap();
    assert_eq!(real(&a), &vec![1.5, -0.25, (1e10f32) as f64]);
    let a = npy::read_npy(&npy_file(1, "<f8", false, "(2,)", &le(&[std::f64::consts::PI, -1e-300], &|x| x.to_le_bytes().to_vec()))).unwrap();
    assert_eq!(real(&a), &vec![std::f64::consts::PI, -1e-300]);

    let a = npy::read_npy(&npy_file(1, "|i1", false, "(3,)", &[0x80, 0xff, 0x7f])).unwrap();
    assert_eq!(real(&a), &vec![-128.0, -1.0, 127.0]);
    let a = npy::read_npy(&npy_file(1, "|u1", false, "(3,)", &[0, 128, 255])).unwrap();
    assert_eq!(real(&a), &vec![0.0, 128.0, 255.0]);
    let a = npy::read_npy(&npy_file(1, "<i2", false, "(2,)", &[0x00, 0x80, 0xff, 0x7f])).unwrap();
    assert_eq!(real(&a), &vec![-32768.0, 32767.0]);
    let a = npy::read_npy(&npy_file(1, "<u2", false, "(1,)", &[0xff, 0xff])).unwrap();
    assert_eq!(real(&a), &vec![65535.0]);
    let a = npy::read_npy(&npy_file(1, "<i4", false, "(2,)", &[(-5i32).to_le_bytes(), 7i32.to_le_bytes()].concat())).unwrap();
    assert_eq!(real(&a), &vec![-5.0, 7.0]);
    let a = npy::read_npy(&npy_file(1, "<u4", false, "(1,)", &u32::MAX.to_le_bytes())).unwrap();
    assert_eq!(real(&a), &vec![4294967295.0]);
    let a = npy::read_npy(&npy_file(1, "<i8", false, "(2,)", &[(-(1i64 << 40)).to_le_bytes(), 123456789012i64.to_le_bytes()].concat())).unwrap();
    assert_eq!(real(&a), &vec![-(1i64 << 40) as f64, 123456789012.0]);
    let a = npy::read_npy(&npy_file(1, "<u8", false, "(1,)", &(1u64 << 52).to_le_bytes())).unwrap();
    assert_eq!(real(&a), &vec![(1u64 << 52) as f64]);

    let a = npy::read_npy(&npy_file(1, "|b1", false, "(3,)", &[1, 0, 1])).unwrap();
    assert_eq!(a.data, NpyData::Bool(vec![true, false, true]));
}

#[test]
fn npy_refuses_int64_it_cannot_hold_exactly() {
    let big = ((1u64 << 53) + 1).to_le_bytes();
    let err = npy::read_npy(&npy_file(1, "<u8", false, "(1,)", &big)).unwrap_err();
    assert!(err.contains("2^53"), "{err}");
    let err = npy::read_npy(&npy_file(1, "<i8", false, "(1,)", &i64::MIN.to_le_bytes())).unwrap_err();
    assert!(err.contains("2^53"), "{err}");
}

#[test]
fn npy_big_endian_and_complex() {
    let be: Vec<u8> = [1.5f64, -2.0].iter().flat_map(|x| x.to_be_bytes()).collect();
    let a = npy::read_npy(&npy_file(1, ">f8", false, "(2,)", &be)).unwrap();
    assert_eq!(real(&a), &vec![1.5, -2.0]);
    let a = npy::read_npy(&npy_file(1, ">i4", false, "(1,)", &(-300i32).to_be_bytes())).unwrap();
    assert_eq!(real(&a), &vec![-300.0]);
    // complex128 (1+2i, -3.5+0i) and complex64 (0.5-1i)
    let c16: Vec<u8> = [1.0f64, 2.0, -3.5, 0.0].iter().flat_map(|x| x.to_le_bytes()).collect();
    let a = npy::read_npy(&npy_file(1, "<c16", false, "(2,)", &c16)).unwrap();
    assert_eq!(a.data, NpyData::Complex(vec![(1.0, 2.0), (-3.5, 0.0)]));
    let c8: Vec<u8> = [0.5f32, -1.0].iter().flat_map(|x| x.to_le_bytes()).collect();
    let a = npy::read_npy(&npy_file(1, "<c8", false, "()", &c8)).unwrap();
    assert_eq!(a.data, NpyData::Complex(vec![(0.5, -1.0)]));
    assert!(a.shape.is_empty());
}

#[test]
fn npy_c_and_fortran_order_agree() {
    // The 2x3 matrix [[1,2,3],[4,5,6]]. C order stores rows; Fortran
    // order stores columns. Both must give the same C-order result.
    let bytes = |v: &[f64]| -> Vec<u8> { v.iter().flat_map(|x| x.to_le_bytes()).collect() };
    let c = npy::read_npy(&npy_file(1, "<f8", false, "(2, 3)", &bytes(&[1., 2., 3., 4., 5., 6.]))).unwrap();
    let f = npy::read_npy(&npy_file(1, "<f8", true, "(2, 3)", &bytes(&[1., 4., 2., 5., 3., 6.]))).unwrap();
    assert_eq!(c, f);
    assert_eq!(c.shape, vec![2, 3]);
    assert_eq!(real(&c), &vec![1., 2., 3., 4., 5., 6.]);
}

#[test]
fn npy_format_versions_2_and_3() {
    let p = 2.5f64.to_le_bytes();
    for major in [1u8, 2, 3] {
        let a = npy::read_npy(&npy_file(major, "<f8", false, "()", &p)).unwrap();
        assert_eq!(real(&a), &vec![2.5], "version {major}");
        assert!(a.shape.is_empty());
    }
    let err = npy::read_npy(&{
        let mut b = npy_file(1, "<f8", false, "()", &p);
        b[6] = 9;
        b
    })
    .unwrap_err();
    assert!(err.contains("version"), "{err}");
}

#[test]
fn npy_refuses_object_string_and_structured_dtypes_by_name() {
    let e = npy::read_npy(&npy_file(1, "|O", false, "(2,)", &[0; 16])).unwrap_err();
    assert!(e.contains("pickle"), "{e}");
    let e = npy::read_npy(&npy_file(1, "<U3", false, "(1,)", &[0; 12])).unwrap_err();
    assert!(e.contains("string dtype"), "{e}");
    let e = npy::read_npy(&npy_file(1, "<M8[ns]", false, "(1,)", &[0; 8])).unwrap_err();
    assert!(e.contains("datetime"), "{e}");
    let mut b = b"\x93NUMPY\x01\x00".to_vec();
    let h = "{'descr': [('a', '<f8'), ('b', '<i4')], 'fortran_order': False, 'shape': (1,), }\n";
    b.extend_from_slice(&(h.len() as u16).to_le_bytes());
    b.extend_from_slice(h.as_bytes());
    b.extend_from_slice(&[0; 12]);
    let e = npy::read_npy(&b).unwrap_err();
    assert!(e.contains("structured"), "{e}");
}

#[test]
fn npy_malformed_inputs_fail_cleanly() {
    let good = npy_file(1, "<f8", false, "(3,)", &[0u8; 24]);
    assert!(npy::read_npy(&good).is_ok());
    // bad magic
    let mut b = good.clone();
    b[0] = 0;
    assert!(npy::read_npy(&b).unwrap_err().contains("magic"));
    // every truncation is an error, never a panic
    for n in 0..good.len() {
        assert!(npy::read_npy(&good[..n]).is_err(), "prefix of {n} bytes was accepted");
    }
    // header length pointing past EOF
    let mut b = good.clone();
    b[8] = 0xff;
    b[9] = 0xff;
    assert!(npy::read_npy(&b).is_err());
    // v2 header length of 4 GiB - 1
    let mut b = npy_file(2, "<f8", false, "(1,)", &[0u8; 8]);
    b[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(npy::read_npy(&b).unwrap_err().contains("implausibly"));
    // a shape that claims more elements than exist, and one that overflows usize
    for shape in ["(1000000000000,)", "(4611686018427387904, 8)", "(65536, 65536, 65536)", "(18446744073709551615,)"] {
        let e = npy::read_npy(&npy_file(1, "<f8", false, shape, &[0u8; 16])).unwrap_err();
        assert!(e.contains("limit") || e.contains("overflow") || e.contains("truncated"), "{shape}: {e}");
    }
    // 1 GiB of uint8 would need 8 GiB decoded -- refused up front even though
    // we only supply the header
    let e = npy::read_npy(&npy_file(1, "|u1", false, "(1073741824,)", &[])).unwrap_err();
    assert!(e.contains("limit"), "{e}");
    // header that is not a dict / missing keys
    let mut b = b"\x93NUMPY\x01\x00".to_vec();
    let h = "garbage\n";
    b.extend_from_slice(&(h.len() as u16).to_le_bytes());
    b.extend_from_slice(h.as_bytes());
    assert!(npy::read_npy(&b).is_err());
    // negative / non-numeric dimension
    assert!(npy::read_npy(&npy_file(1, "<f8", false, "(-3,)", &[])).is_err());
    // too many dimensions
    let many = format!("({})", vec!["1"; 70].join(", "));
    assert!(npy::read_npy(&npy_file(1, "<f8", false, &many, &[0; 8])).is_err());
}

#[test]
fn npy_round_trip_is_bit_exact() {
    let vals = vec![0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e-310, f64::MAX, -1.0 / 3.0];
    let arr = NpyArray { shape: vec![vals.len()], data: NpyData::Real(vals.clone()) };
    let back = npy::read_npy(&npy::write_npy(&arr)).unwrap();
    let got = real(&back);
    for (a, b) in vals.iter().zip(got) {
        assert_eq!(a.to_bits(), b.to_bits(), "{a} vs {b}");
    }
    // header is 64-byte aligned like NumPy's own
    let bytes = npy::write_npy(&arr);
    let hl = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
    assert_eq!((10 + hl) % 64, 0);
    assert_eq!(bytes[10 + hl - 1], b'\n');
}

fn tmp(name: &str) -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("qu_interop_{}_{name}", std::process::id()));
    p.to_string_lossy().replace('\\', "/")
}

/// Plain-text view of a value (`Value` has no `Display`).
fn show(v: &qu_interp::Value) -> String {
    match v {
        qu_interp::Value::Num(n) => format!("{n}"),
        qu_interp::Value::Str(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn run(it: &mut Interp, src: &str) {
    it.run(src).unwrap_or_else(|e| panic!("script failed: {e}\n{src}"));
}

#[test]
fn script_round_trip_npy_values_and_matrix() {
    let path = tmp("rt.npy");
    let mut it = Interp::new();
    run(&mut it, &format!(
        r#"
x = [0, -0.0, 1.5, inf, -inf, nan]
write_npy("{path}", x)
y = read_npy("{path}")
"#));
    // compare bit patterns in Rust: -0.0 and NaN must survive exactly
    let (Some(qu_interp::Value::Vec(x)), Some(qu_interp::Value::Vec(y))) = (it.get("x"), it.get("y")) else {
        panic!("x and y should be vectors")
    };
    assert_eq!(x.len(), 6);
    assert_eq!(x.len(), y.len());
    for (a, b) in x.iter().zip(y.iter()) {
        assert_eq!(a.to_bits(), b.to_bits(), "{a} vs {b}");
    }
    assert_eq!(y[1].to_bits(), (-0.0f64).to_bits());

    // 2-d: shape and orientation survive (C order on disk, column-major in Qu)
    let mpath = tmp("m.npy");
    run(&mut it, &format!(
        r#"
M = [1, 2, 3; 4, 5, 6]
write_npy("{mpath}", M)
N = read_npy("{mpath}")
r = nrow(N)
c = ncol(N)
v = N[0, 2]
w = N[1, 0]
"#));
    assert_eq!(show(it.get("r").unwrap()), "2");
    assert_eq!(show(it.get("c").unwrap()), "3");
    assert_eq!(show(it.get("v").unwrap()), "3");
    assert_eq!(show(it.get("w").unwrap()), "4");
    // and the bytes on disk are what NumPy would see: row-major
    let bytes = std::fs::read(&mpath).unwrap();
    let a = npy::read_npy(&bytes).unwrap();
    assert_eq!(a.shape, vec![2, 3]);
    assert_eq!(real(&a), &vec![1., 2., 3., 4., 5., 6.]);
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(mpath);
}

#[test]
fn script_reads_numpy_made_file_and_rejects_3d() {
    let p3 = tmp("three.npy");
    std::fs::write(&p3, npy_file(1, "<f8", false, "(2, 2, 2)", &[0u8; 64])).unwrap();
    let mut it = Interp::new();
    let err = it.run(&format!("x = read_npy(\"{p3}\")")).unwrap_err().to_string();
    assert!(err.contains("[2, 2, 2]"), "{err}");
    let _ = std::fs::remove_file(p3);

    let p0 = tmp("scalar.npy");
    std::fs::write(&p0, npy_file(1, "<f8", false, "()", &42.0f64.to_le_bytes())).unwrap();
    run(&mut it, &format!("s = read_npy(\"{p0}\")"));
    assert_eq!(show(it.get("s").unwrap()), "42");
    let _ = std::fs::remove_file(p0);

    let err = Interp::new().run("x = read_npy(\"/definitely/not/here.npy\")").unwrap_err().to_string();
    assert!(err.contains("read_npy"), "{err}");
}

// ================================================================== NPZ

/// A zip with one member using method `method` (0 stored, 8 raw-deflate of
/// a single stored block) built by hand, independent of `npy::write_zip`.
fn zip_one(name: &str, data: &[u8], method: u16, crc: u32, claimed_usize: u32) -> Vec<u8> {
    let payload: Vec<u8> = if method == 8 {
        let mut p = vec![0x01];
        p.extend_from_slice(&(data.len() as u16).to_le_bytes());
        p.extend_from_slice(&(!(data.len() as u16)).to_le_bytes());
        p.extend_from_slice(data);
        p
    } else {
        data.to_vec()
    };
    let mut out = Vec::new();
    out.extend_from_slice(b"PK\x03\x04");
    out.extend_from_slice(&[20, 0, 0, 0]);
    out.extend_from_slice(&method.to_le_bytes());
    out.extend_from_slice(&[0, 0, 33, 0]);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&claimed_usize.to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&payload);
    let cd_off = out.len() as u32;
    out.extend_from_slice(b"PK\x01\x02");
    out.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
    out.extend_from_slice(&method.to_le_bytes());
    out.extend_from_slice(&[0, 0, 33, 0]);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&claimed_usize.to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    let cd_size = out.len() as u32 - cd_off;
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[test]
fn crc32_matches_the_standard_vector() {
    assert_eq!(npy::crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(npy::crc32(b""), 0);
}

#[test]
fn npz_reads_stored_and_deflate_members_and_round_trips() {
    let member = npy_file(1, "<f8", false, "(2,)", &[1.0f64, 2.0].iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>());
    for method in [0u16, 8] {
        let z = zip_one("a.npy", &member, method, npy::crc32(&member), member.len() as u32);
        let arrays = npy::read_npz(&z).unwrap();
        assert_eq!(arrays.len(), 1);
        assert_eq!(arrays[0].0, "a");
        assert_eq!(real(&arrays[0].1), &vec![1.0, 2.0]);
    }

    let arrays = vec![
        ("x".to_string(), NpyArray { shape: vec![3], data: NpyData::Real(vec![1.0, f64::NAN, -0.0]) }),
        ("flag".to_string(), NpyArray { shape: vec![], data: NpyData::Bool(vec![true]) }),
        ("z".to_string(), NpyArray { shape: vec![1, 2], data: NpyData::Complex(vec![(1.0, 2.0), (3.0, 4.0)]) }),
    ];
    let back = npy::read_npz(&npy::write_npz(&arrays).unwrap()).unwrap();
    assert_eq!(back.len(), 3);
    assert_eq!(back[1], arrays[1]);
    assert_eq!(back[2], arrays[2]);
    assert_eq!(real(&back[0].1)[0], 1.0);
    assert!(real(&back[0].1)[1].is_nan());
    assert_eq!(real(&back[0].1)[2].to_bits(), (-0.0f64).to_bits());
}

#[test]
fn script_npz_round_trip_to_a_record() {
    let path = tmp("rt.npz");
    let mut it = Interp::new();
    run(&mut it, &format!(
        r#"
write_npz("{path}", {{a = [1, 2, 3], m = [1, 2; 3, 4], s = 2.5}})
r = read_npz("{path}")
la = length(r.a)
m10 = r.m[1, 0]
s = r.s
"#));
    assert_eq!(show(it.get("la").unwrap()), "3");
    assert_eq!(show(it.get("m10").unwrap()), "3");
    assert_eq!(show(it.get("s").unwrap()), "2.5");
    let _ = std::fs::remove_file(path);
}

#[test]
fn npz_malformed_inputs_fail_cleanly() {
    let member = npy_file(1, "<f8", false, "(1,)", &1.0f64.to_le_bytes());
    let good = zip_one("a.npy", &member, 0, npy::crc32(&member), member.len() as u32);
    assert!(npy::read_npz(&good).is_ok());
    // every truncation is an error
    for n in 0..good.len() {
        assert!(npy::read_npz(&good[..n]).is_err(), "prefix {n} accepted");
    }
    // bad CRC
    let bad = zip_one("a.npy", &member, 0, 0xdead_beef, member.len() as u32);
    assert!(npy::read_npz(&bad).unwrap_err().contains("CRC"));
    // a member that claims to be 4 GB uncompressed (deflate bomb shape)
    let bomb = zip_one("a.npy", &member, 8, 0, u32::MAX - 1);
    assert!(npy::read_npz(&bomb).unwrap_err().contains("limit"));
    // central directory offset past EOF
    let mut b = good.clone();
    let n = b.len();
    b[n - 6..n - 2].copy_from_slice(&0x7fff_0000u32.to_le_bytes());
    assert!(npy::read_npz(&b).is_err());
    // entry count larger than the directory can hold
    let mut b = good.clone();
    b[n - 12..n - 10].copy_from_slice(&60000u16.to_le_bytes());
    b[n - 14..n - 12].copy_from_slice(&60000u16.to_le_bytes());
    assert!(npy::read_npz(&b).is_err());
    // unsupported compression method is named
    let m = zip_one("a.npy", &member, 12, 0, member.len() as u32);
    assert!(npy::read_npz(&m).unwrap_err().contains("method 12"));
    // not a zip at all
    assert!(npy::read_npz(b"hello world, definitely not a zip file").is_err());
    // a valid zip whose member is not a valid .npy names the member
    let junk = zip_one("a.npy", b"zzzz", 0, npy::crc32(b"zzzz"), 4);
    assert!(npy::read_npz(&junk).unwrap_err().contains("a.npy"));
}

// ================================================================= HDF5
//
// A minimal HDF5 writer: superblock v0 + v1 object headers + old-style
// groups (symbol table), and a v2 variant (superblock v2, "OHDR" headers,
// link messages). Offsets and lengths are 8 bytes.

const UNDEF: u64 = u64::MAX;

fn pad8(v: &mut Vec<u8>) {
    while v.len() % 8 != 0 {
        v.push(0);
    }
}
fn p16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn p32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn p64(v: &mut Vec<u8>, x: u64) {
    v.extend_from_slice(&x.to_le_bytes());
}

fn dt_float(size: u32, big: bool) -> Vec<u8> {
    let (sign, eloc, esz, mloc, msz, bias) = match size {
        4 => (31u8, 23u8, 8u8, 0u8, 23u8, 127u32),
        8 => (63, 52, 11, 0, 52, 1023),
        _ => panic!(),
    };
    let mut v = vec![0x11, 0x20 | big as u8, sign, 0];
    p32(&mut v, size);
    p16(&mut v, 0);
    p16(&mut v, (size * 8) as u16);
    v.extend_from_slice(&[eloc, esz, mloc, msz]);
    p32(&mut v, bias);
    v
}
fn dt_int(size: u32, signed: bool, big: bool) -> Vec<u8> {
    let mut v = vec![0x10, (signed as u8) << 3 | big as u8, 0, 0];
    p32(&mut v, size);
    p16(&mut v, 0);
    p16(&mut v, (size * 8) as u16);
    v
}
fn dt_string(size: u32) -> Vec<u8> {
    let mut v = vec![0x13, 0, 0, 0];
    p32(&mut v, size);
    v
}
fn dt_vlen_string() -> Vec<u8> {
    let mut v = vec![0x19, 1, 0, 0];
    p32(&mut v, 16);
    v.extend_from_slice(&dt_string(1)); // base type
    v
}
fn dt_compound() -> Vec<u8> {
    let mut v = vec![0x16, 0, 0, 0];
    p32(&mut v, 8);
    v
}

fn space_v1(shape: &[u64]) -> Vec<u8> {
    let mut v = vec![1, shape.len() as u8, 0, 0, 0, 0, 0, 0];
    for &d in shape {
        p64(&mut v, d);
    }
    v
}
fn space_v2(shape: &[u64]) -> Vec<u8> {
    let mut v = vec![2, shape.len() as u8, 0, if shape.is_empty() { 0 } else { 1 }];
    for &d in shape {
        p64(&mut v, d);
    }
    v
}

fn layout_contig(addr: u64, size: u64) -> Vec<u8> {
    let mut v = vec![3, 1];
    p64(&mut v, addr);
    p64(&mut v, size);
    v
}
fn layout_chunked(btree: u64, chunk: &[u32], elem: u32) -> Vec<u8> {
    let mut v = vec![3, 2, (chunk.len() + 1) as u8];
    p64(&mut v, btree);
    for &c in chunk {
        p32(&mut v, c);
    }
    p32(&mut v, elem);
    v
}
fn layout_compact(data: &[u8]) -> Vec<u8> {
    let mut v = vec![3, 0];
    p16(&mut v, data.len() as u16);
    v.extend_from_slice(data);
    v
}
fn filters_v1(list: &[(u16, &[u32])]) -> Vec<u8> {
    let mut v = vec![1, list.len() as u8, 0, 0, 0, 0, 0, 0];
    for (id, cd) in list {
        p16(&mut v, *id);
        p16(&mut v, 0);
        p16(&mut v, 0);
        p16(&mut v, cd.len() as u16);
        for c in *cd {
            p32(&mut v, *c);
        }
        if cd.len() % 2 == 1 {
            p32(&mut v, 0);
        }
    }
    v
}

fn adler32(d: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in d {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
/// zlib stream made of stored DEFLATE blocks.
fn zlib_stored(d: &[u8]) -> Vec<u8> {
    let mut v = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = if d.is_empty() { vec![&[][..]] } else { d.chunks(65535).collect() };
    for (i, b) in blocks.iter().enumerate() {
        v.push((i == blocks.len() - 1) as u8);
        v.extend_from_slice(&(b.len() as u16).to_le_bytes());
        v.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        v.extend_from_slice(b);
    }
    v.extend_from_slice(&adler32(d).to_be_bytes());
    v
}
fn shuffle(d: &[u8], e: usize) -> Vec<u8> {
    let n = d.len() / e;
    let mut out = vec![0u8; d.len()];
    for i in 0..n {
        for j in 0..e {
            out[j * n + i] = d[i * e + j];
        }
    }
    out
}

struct B {
    buf: Vec<u8>,
    v2: bool,
}
impl B {
    fn new(v2: bool) -> Self {
        B { buf: vec![0u8; if v2 { 48 } else { 96 }], v2 }
    }
    fn put(&mut self, bytes: &[u8]) -> u64 {
        pad8(&mut self.buf);
        let a = self.buf.len() as u64;
        self.buf.extend_from_slice(bytes);
        a
    }
    /// An object header holding `msgs` (type, payload).
    fn header(&mut self, msgs: &[(u16, Vec<u8>)]) -> u64 {
        let mut body = Vec::new();
        let mut h = Vec::new();
        if self.v2 {
            for (ty, data) in msgs {
                let mut d = data.clone();
                // v2 messages need no padding; keep 8-alignment anyway
                body.push(*ty as u8);
                p16(&mut body, d.len() as u16);
                body.push(0);
                body.append(&mut d);
            }
            h.extend_from_slice(b"OHDR");
            h.push(2);
            h.push(0x02); // 4-byte chunk size
            p32(&mut h, body.len() as u32);
            h.extend_from_slice(&body);
            p32(&mut h, 0); // checksum (not verified)
        } else {
            for (ty, data) in msgs {
                let mut d = data.clone();
                pad8(&mut d);
                p16(&mut body, *ty);
                p16(&mut body, d.len() as u16);
                body.extend_from_slice(&[0, 0, 0, 0]);
                body.append(&mut d);
            }
            h.push(1);
            h.push(0);
            p16(&mut h, msgs.len() as u16);
            p32(&mut h, 1);
            p32(&mut h, body.len() as u32);
            p32(&mut h, 0);
            h.extend_from_slice(&body);
        }
        self.put(&h)
    }
    fn dataset(&mut self, dt: Vec<u8>, shape: &[u64], layout: Vec<u8>, extra: Vec<(u16, Vec<u8>)>) -> u64 {
        let sp = if self.v2 { space_v2(shape) } else { space_v1(shape) };
        let mut msgs = vec![(0x01, sp), (0x03, dt), (0x08, layout)];
        msgs.extend(extra);
        self.header(&msgs)
    }
    fn contiguous(&mut self, dt: Vec<u8>, shape: &[u64], raw: &[u8]) -> u64 {
        let a = self.put(raw);
        self.dataset(dt, shape, layout_contig(a, raw.len() as u64), vec![])
    }
    /// A group with the given members, old style (symbol table) or, in a v2
    /// file, hard-link messages.
    fn group(&mut self, members: &[(&str, u64)]) -> u64 {
        if self.v2 {
            let mut msgs = Vec::new();
            for (name, addr) in members {
                let mut l = vec![1, 0, name.len() as u8];
                l.extend_from_slice(name.as_bytes());
                p64(&mut l, *addr);
                msgs.push((0x06u16, l));
            }
            return self.header(&msgs);
        }
        // local heap data: empty string at 0, then names
        let mut heap = vec![0u8; 8];
        let mut offs = Vec::new();
        for (name, _) in members {
            offs.push(heap.len() as u64);
            heap.extend_from_slice(name.as_bytes());
            heap.push(0);
            pad8(&mut heap);
        }
        let heap_data = self.put(&heap);
        let mut hh = b"HEAP".to_vec();
        hh.extend_from_slice(&[1, 0, 0, 0]);
        p64(&mut hh, heap.len() as u64);
        p64(&mut hh, 1); // free list: none
        p64(&mut hh, heap_data);
        let heap_hdr = self.put(&hh);
        // symbol table node
        let mut sn = b"SNOD".to_vec();
        sn.extend_from_slice(&[1, 0]);
        p16(&mut sn, members.len() as u16);
        for (i, (_, addr)) in members.iter().enumerate() {
            p64(&mut sn, offs[i]);
            p64(&mut sn, *addr);
            p32(&mut sn, 0);
            p32(&mut sn, 0);
            sn.extend_from_slice(&[0u8; 16]);
        }
        let snod = self.put(&sn);
        // B-tree with one leaf entry
        let mut bt = b"TREE".to_vec();
        bt.extend_from_slice(&[0, 0]);
        p16(&mut bt, 1);
        p64(&mut bt, UNDEF);
        p64(&mut bt, UNDEF);
        p64(&mut bt, 0);
        p64(&mut bt, snod);
        p64(&mut bt, *offs.last().unwrap_or(&0));
        let btree = self.put(&bt);
        let mut st = Vec::new();
        p64(&mut st, btree);
        p64(&mut st, heap_hdr);
        self.header(&[(0x11, st)])
    }
    fn finish(mut self, root: u64) -> Vec<u8> {
        pad8(&mut self.buf);
        let eof = self.buf.len() as u64;
        let mut sb = SIG.to_vec();
        if self.v2 {
            sb.extend_from_slice(&[2, 8, 8, 0]);
            p64(&mut sb, 0);
            p64(&mut sb, UNDEF);
            p64(&mut sb, eof);
            p64(&mut sb, root);
            p32(&mut sb, 0);
        } else {
            sb.extend_from_slice(&[0, 0, 0, 0, 0, 8, 8, 0]);
            p16(&mut sb, 4);
            p16(&mut sb, 16);
            p32(&mut sb, 0);
            p64(&mut sb, 0);
            p64(&mut sb, UNDEF);
            p64(&mut sb, eof);
            p64(&mut sb, UNDEF);
            // root symbol table entry
            p64(&mut sb, 0);
            p64(&mut sb, root);
            p32(&mut sb, 0);
            p32(&mut sb, 0);
            sb.extend_from_slice(&[0u8; 16]);
        }
        let n = sb.len();
        self.buf[..n].copy_from_slice(&sb);
        self.buf
    }
}
const SIG: &[u8; 8] = b"\x89HDF\r\n\x1a\n";

fn f64s(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The chunked, shuffled, deflated 5x7 dataset: value(i, j) = 10 i + j.
fn add_chunked(b: &mut B) -> u64 {
    let (rows, cols, cr, cc) = (5usize, 7usize, 2usize, 3usize);
    let mut entries = Vec::new();
    for ci in 0..rows.div_ceil(cr) {
        for cj in 0..cols.div_ceil(cc) {
            let mut raw = Vec::new();
            for a in 0..cr {
                for c in 0..cc {
                    let (i, j) = (ci * cr + a, cj * cc + c);
                    let v = if i < rows && j < cols { (10 * i + j) as f64 } else { 0.0 };
                    raw.extend_from_slice(&v.to_le_bytes());
                }
            }
            let stored = zlib_stored(&shuffle(&raw, 8));
            let addr = b.put(&stored);
            entries.push((stored.len() as u32, [ci * cr, cj * cc], addr));
        }
    }
    let mut bt = b"TREE".to_vec();
    bt.extend_from_slice(&[1, 0]);
    p16(&mut bt, entries.len() as u16);
    p64(&mut bt, UNDEF);
    p64(&mut bt, UNDEF);
    for (size, off, addr) in &entries {
        p32(&mut bt, *size);
        p32(&mut bt, 0);
        p64(&mut bt, off[0] as u64);
        p64(&mut bt, off[1] as u64);
        p64(&mut bt, 0);
        p64(&mut bt, *addr);
    }
    // final key
    p32(&mut bt, 0);
    p32(&mut bt, 0);
    p64(&mut bt, 6);
    p64(&mut bt, 9);
    p64(&mut bt, 0);
    let btree = b.put(&bt);
    b.dataset(
        dt_float(8, false),
        &[rows as u64, cols as u64],
        layout_chunked(btree, &[cr as u32, cc as u32], 8),
        vec![(0x0b, filters_v1(&[(2, &[8]), (1, &[6])]))],
    )
}

/// The reference old-style file used by most tests.
fn sample_file() -> Vec<u8> {
    let mut b = B::new(false);
    let a = b.contiguous(dt_float(8, false), &[4], &f64s(&[1.5, -2.0, f64::NAN, f64::NEG_INFINITY]));
    let m = b.contiguous(
        dt_int(4, true, false),
        &[2, 3],
        &[1i32, 2, 3, 4, 5, 6].iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>(),
    );
    let s = b.contiguous(dt_string(8), &[], b"hello\0\0\0");
    let sc = b.contiguous(dt_float(8, false), &[], &f64s(&[6.25]));
    let big = b.contiguous(dt_int(2, true, true), &[3], &[0xff, 0xfe, 0x00, 0x05, 0x7f, 0xff]);
    let u8d = b.contiguous(dt_int(1, false, false), &[3], &[0, 200, 255]);
    let f32d = b.contiguous(dt_float(4, false), &[2], &[0.5f32, -3.0].iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>());
    let compact = b.dataset(dt_int(1, false, false), &[2], layout_compact(&[7, 9]), vec![]);
    let chunked = add_chunked(&mut b);
    let comp = b.dataset(dt_compound(), &[1], layout_contig(UNDEF, 0), vec![]);
    let strs = b.contiguous(dt_string(3), &[3], b"ab\0cd\0e  ");
    let inner = b.group(&[("m", m), ("s", s)]);
    let deeper = b.group(&[("sc", sc)]);
    let grp = b.group(&[("deep", deeper), ("inner", inner)]);
    let root = b.group(&[
        ("a", a),
        ("big", big),
        ("chunked", chunked),
        ("compact", compact),
        ("comp", comp),
        ("f32", f32d),
        ("grp", grp),
        ("strs", strs),
        ("u8", u8d),
    ]);
    b.finish(root)
}

fn h5(bytes: &[u8], ds: &str) -> Result<H5Value, String> {
    hdf5::h5read(bytes, ds)
}
fn real_of(v: H5Value) -> (Vec<usize>, Vec<f64>) {
    match v {
        H5Value::Real { shape, data } => (shape, data),
        other => panic!("expected numbers, got {other:?}"),
    }
}

#[test]
fn hdf5_reads_contiguous_datasets_of_each_type() {
    let f = sample_file();
    let (shape, d) = real_of(h5(&f, "/a").unwrap());
    assert_eq!(shape, vec![4]);
    assert_eq!(d[0], 1.5);
    assert_eq!(d[1], -2.0);
    assert!(d[2].is_nan());
    assert_eq!(d[3], f64::NEG_INFINITY);

    let (shape, d) = real_of(h5(&f, "/grp/inner/m").unwrap());
    assert_eq!(shape, vec![2, 3]);
    assert_eq!(d, vec![1., 2., 3., 4., 5., 6.]);

    // leading slash optional
    let (_, d) = real_of(h5(&f, "grp/deep/sc").unwrap());
    assert_eq!(d, vec![6.25]);
    let (shape, _) = real_of(h5(&f, "/grp/deep/sc").unwrap());
    assert!(shape.is_empty());

    // big-endian int16: 0xfffe = -2, 0x0005 = 5, 0x7fff = 32767
    let (_, d) = real_of(h5(&f, "/big").unwrap());
    assert_eq!(d, vec![-2.0, 5.0, 32767.0]);
    let (_, d) = real_of(h5(&f, "/u8").unwrap());
    assert_eq!(d, vec![0.0, 200.0, 255.0]);
    let (_, d) = real_of(h5(&f, "/f32").unwrap());
    assert_eq!(d, vec![0.5, -3.0]);
    let (_, d) = real_of(h5(&f, "/compact").unwrap());
    assert_eq!(d, vec![7.0, 9.0]);
}

#[test]
fn hdf5_reads_strings() {
    let f = sample_file();
    match h5(&f, "/grp/inner/s").unwrap() {
        H5Value::Text { shape, data } => {
            assert!(shape.is_empty());
            assert_eq!(data, vec!["hello".to_string()]);
        }
        o => panic!("{o:?}"),
    }
    match h5(&f, "/strs").unwrap() {
        H5Value::Text { shape, data } => {
            assert_eq!(shape, vec![3]);
            assert_eq!(data, vec!["ab", "cd", "e"]);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn hdf5_reads_chunked_shuffled_deflated_with_edge_chunks() {
    let f = sample_file();
    let (shape, d) = real_of(h5(&f, "/chunked").unwrap());
    assert_eq!(shape, vec![5, 7]);
    for i in 0..5 {
        for j in 0..7 {
            assert_eq!(d[i * 7 + j], (10 * i + j) as f64, "element ({i},{j})");
        }
    }
}

#[test]
fn hdf5_h5info_lists_groups_and_datasets() {
    let f = sample_file();
    let info = hdf5::h5info(&f).unwrap();
    let find = |p: &str| info.iter().find(|e| e.path == p).unwrap_or_else(|| panic!("{p} missing in {info:?}"));
    assert_eq!(find("/grp").kind, "group");
    assert_eq!(find("/grp/inner").kind, "group");
    let m = find("/grp/inner/m");
    assert_eq!((m.kind, m.dtype.as_str(), m.shape.clone()), ("dataset", "int32", vec![2, 3]));
    let a = find("/a");
    assert_eq!((a.dtype.as_str(), a.shape.clone()), ("float64", vec![4]));
    assert_eq!(find("/big").dtype, "int16");
    assert_eq!(find("/u8").dtype, "uint8");
    assert_eq!(find("/strs").dtype, "string[3]");
    assert!(find("/comp").dtype.contains("compound"));
    assert_eq!(find("/chunked").shape, vec![5, 7]);
}

#[test]
fn hdf5_unsupported_features_are_named_not_misread() {
    let f = sample_file();
    let e = h5(&f, "/comp").unwrap_err();
    assert!(e.contains("compound"), "{e}");
    let e = h5(&f, "/grp").unwrap_err();
    assert!(e.contains("group"), "{e}");
    let e = h5(&f, "/nope").unwrap_err();
    assert!(e.contains("not found"), "{e}");
    let e = h5(&f, "/a/b").unwrap_err();
    assert!(e.contains("not a group"), "{e}");

    // an LZF-filtered chunked dataset
    let mut b = B::new(false);
    let e = {
        let raw = b.put(&[0u8; 8]);
        let mut bt = b"TREE".to_vec();
        bt.extend_from_slice(&[1, 0]);
        p16(&mut bt, 1);
        p64(&mut bt, UNDEF);
        p64(&mut bt, UNDEF);
        p32(&mut bt, 8);
        p32(&mut bt, 0);
        p64(&mut bt, 0);
        p64(&mut bt, 0);
        p64(&mut bt, raw);
        p32(&mut bt, 0);
        p32(&mut bt, 0);
        p64(&mut bt, 1);
        p64(&mut bt, 0);
        let btree = b.put(&bt);
        let d = b.dataset(
            dt_float(8, false),
            &[1],
            layout_chunked(btree, &[1], 8),
            vec![(0x0b, filters_v1(&[(32000, &[])]))],
        );
        let root = b.group(&[("lzf", d)]);
        let file = b.finish(root);
        h5(&file, "/lzf").unwrap_err()
    };
    assert!(e.contains("LZF") && e.contains("32000"), "{e}");
}

#[test]
fn hdf5_v2_superblock_object_headers_and_link_messages() {
    let mut b = B::new(true);
    let a = b.contiguous(dt_float(8, false), &[3], &f64s(&[1.0, 2.0, 3.0]));
    let s = b.contiguous(dt_float(8, false), &[], &f64s(&[9.5]));
    let g = b.group(&[("s", s)]);
    let root = b.group(&[("a", a), ("g", g)]);
    let f = b.finish(root);
    let (shape, d) = real_of(h5(&f, "/a").unwrap());
    assert_eq!((shape, d), (vec![3], vec![1.0, 2.0, 3.0]));
    let (_, d) = real_of(h5(&f, "/g/s").unwrap());
    assert_eq!(d, vec![9.5]);
    let info = hdf5::h5info(&f).unwrap();
    assert_eq!(info.len(), 3);
    assert!(info.iter().any(|e| e.path == "/g/s" && e.kind == "dataset"));
}

#[test]
fn hdf5_v1_header_continuation_block() {
    // The layout message lives in a continuation block.
    let mut b = B::new(false);
    let raw = b.put(&f64s(&[4.0, 5.0]));
    let mut cont_msg = Vec::new();
    let mut block = Vec::new();
    let lay = {
        let mut d = layout_contig(raw, 16);
        pad8(&mut d);
        d
    };
    p16(&mut block, 0x08);
    p16(&mut block, lay.len() as u16);
    block.extend_from_slice(&[0, 0, 0, 0]);
    block.extend_from_slice(&lay);
    let block_addr = b.put(&block);
    p64(&mut cont_msg, block_addr);
    p64(&mut cont_msg, block.len() as u64);
    let d = b.header(&[(0x01, space_v1(&[2])), (0x03, dt_float(8, false)), (0x10, cont_msg)]);
    let root = b.group(&[("d", d)]);
    let f = b.finish(root);
    let (_, v) = real_of(h5(&f, "/d").unwrap());
    assert_eq!(v, vec![4.0, 5.0]);
}

#[test]
fn hdf5_unallocated_storage_reads_as_fill() {
    let mut b = B::new(false);
    let d = b.dataset(dt_float(8, false), &[3], layout_contig(UNDEF, 0), vec![]);
    let root = b.group(&[("d", d)]);
    let f = b.finish(root);
    let (_, v) = real_of(h5(&f, "/d").unwrap());
    assert_eq!(v, vec![0.0; 3]);
}

#[test]
fn hdf5_variable_length_strings_via_global_heap() {
    let mut b = B::new(false);
    // global heap collection with two objects
    let mut g = b"GCOL".to_vec();
    g.extend_from_slice(&[1, 0, 0, 0]);
    let mut objs = Vec::new();
    for (idx, text) in [(1u16, "alpha"), (2u16, "be")] {
        p16(&mut objs, idx);
        p16(&mut objs, 1);
        p32(&mut objs, 0);
        p64(&mut objs, text.len() as u64);
        objs.extend_from_slice(text.as_bytes());
        pad8(&mut objs);
    }
    p16(&mut objs, 0); // terminator
    pad8(&mut objs);
    p64(&mut g, (16 + objs.len()) as u64);
    g.extend_from_slice(&objs);
    let gaddr = b.put(&g);
    let mut elems = Vec::new();
    for (len, idx) in [(5u32, 1u32), (2, 2)] {
        p32(&mut elems, len);
        p64(&mut elems, gaddr);
        p32(&mut elems, idx);
    }
    let d = b.contiguous(dt_vlen_string(), &[2], &elems);
    let root = b.group(&[("v", d)]);
    let f = b.finish(root);
    match h5(&f, "/v").unwrap() {
        H5Value::Text { data, .. } => assert_eq!(data, vec!["alpha", "be"]),
        o => panic!("{o:?}"),
    }
}

#[test]
fn hdf5_malformed_inputs_fail_cleanly() {
    let f = sample_file();
    // bad magic
    assert!(hdf5::h5info(b"this is not an hdf5 file at all, just text").unwrap_err().contains("not an HDF5"));
    assert!(hdf5::h5info(&[]).is_err());
    // every truncation: an error or a result, never a panic or a hang
    let mut n = 0;
    while n < f.len() {
        let _ = hdf5::h5info(&f[..n]);
        for ds in ["/a", "/chunked", "/grp/inner/m", "/strs"] {
            let _ = hdf5::h5read(&f[..n], ds);
        }
        n += 5;
    }
    assert!(hdf5::h5read(&f[..60], "/a").is_err());
    // cut in the middle of the file: whatever sits past the cut is unreadable
    assert!(hdf5::h5read(&f[..f.len() / 2], "/strs").is_err());
}

#[test]
fn hdf5_huge_shape_and_offsets_past_eof_are_refused() {
    // dataset whose dataspace claims 2^40 x 2^20 elements
    let mut b = B::new(false);
    let d = b.contiguous(dt_float(8, false), &[1 << 40, 1 << 20], &[0u8; 16]);
    let root = b.group(&[("d", d)]);
    let e = h5(&b.finish(root), "/d").unwrap_err();
    assert!(e.contains("limit") || e.contains("overflow"), "{e}");

    // 2^31 uint8 elements would be 16 GiB once decoded
    let mut b = B::new(false);
    let d = b.dataset(dt_int(1, false, false), &[1 << 31], layout_contig(UNDEF, 0), vec![]);
    let root = b.group(&[("d", d)]);
    let e = h5(&b.finish(root), "/d").unwrap_err();
    assert!(e.contains("limit"), "{e}");

    // contiguous address past the end of the file
    let mut b = B::new(false);
    let d = b.dataset(dt_float(8, false), &[2], layout_contig(1 << 40, 16), vec![]);
    let root = b.group(&[("d", d)]);
    let e = h5(&b.finish(root), "/d").unwrap_err();
    assert!(e.contains("past the end"), "{e}");

    // contiguous storage smaller than the shape needs
    let mut b = B::new(false);
    let raw = b.put(&[0u8; 8]);
    let d = b.dataset(dt_float(8, false), &[100], layout_contig(raw, 8), vec![]);
    let root = b.group(&[("d", d)]);
    assert!(h5(&b.finish(root), "/d").is_err());

    // root group address past EOF
    let mut f = sample_file();
    let at = 24 + 4 * 8 + 8;
    f[at..at + 8].copy_from_slice(&(1u64 << 50).to_le_bytes());
    assert!(hdf5::h5info(&f).is_err());

    // a deflate bomb in a chunk: claims tiny chunk, stream would expand
    // (a stored-block stream bigger than the declared chunk)
    let mut b = B::new(false);
    let big = zlib_stored(&vec![0u8; 4096]);
    let raw = b.put(&big);
    let mut bt = b"TREE".to_vec();
    bt.extend_from_slice(&[1, 0]);
    p16(&mut bt, 1);
    p64(&mut bt, UNDEF);
    p64(&mut bt, UNDEF);
    p32(&mut bt, big.len() as u32);
    p32(&mut bt, 0);
    p64(&mut bt, 0);
    p64(&mut bt, 0);
    p64(&mut bt, raw);
    p32(&mut bt, 0);
    p32(&mut bt, 0);
    p64(&mut bt, 1);
    p64(&mut bt, 0);
    let btree = b.put(&bt);
    let d = b.dataset(
        dt_float(8, false),
        &[1],
        layout_chunked(btree, &[1], 8),
        vec![(0x0b, filters_v1(&[(1, &[6])]))],
    );
    let root = b.group(&[("d", d)]);
    let e = h5(&b.finish(root), "/d").unwrap_err();
    assert!(e.contains("limit"), "{e}");
}

#[test]
fn hdf5_cyclic_group_terminates() {
    // root's member "loop" is the root group itself
    let mut b = B::new(true);
    let placeholder = b.put(&[0u8; 8]);
    let root = b.group(&[("loop", placeholder)]);
    // patch the link to point at root
    let mut f = b.finish(root);
    let needle = placeholder.to_le_bytes();
    let pos = f
        .windows(8)
        .rposition(|w| w == needle)
        .expect("link address present");
    f[pos..pos + 8].copy_from_slice(&root.to_le_bytes());
    let info = hdf5::h5info(&f).unwrap();
    assert_eq!(info.len(), 1);
}

#[test]
fn hdf5_random_corruption_never_panics() {
    let f = sample_file();
    let mut seed = 0x1234_5678_9abc_def0u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..1500 {
        let mut g = f.clone();
        for _ in 0..(1 + next() % 4) {
            let i = (next() as usize) % g.len();
            g[i] = next() as u8;
        }
        let _ = hdf5::h5info(&g);
        let _ = hdf5::h5read(&g, "/chunked");
        let _ = hdf5::h5read(&g, "/grp/inner/m");
        let _ = hdf5::h5read(&g, "/strs");
    }
}

#[test]
fn script_h5read_and_h5info() {
    let path = tmp("sample.h5");
    std::fs::write(&path, sample_file()).unwrap();
    let mut it = Interp::new();
    run(&mut it, &format!(
        r#"
a = h5read("{path}", "/a")
m = h5read("{path}", "/grp/inner/m")
s = h5read("{path}", "/grp/inner/s")
c = h5read("{path}", "/chunked")
info = h5info("{path}")
n = length(info)
nr = nrow(m)
m12 = m[1, 2]
c43 = c[4, 3]
la = length(a)
"#));
    assert_eq!(show(it.get("la").unwrap()), "4");
    assert_eq!(show(it.get("nr").unwrap()), "2");
    assert_eq!(show(it.get("m12").unwrap()), "6");
    assert_eq!(show(it.get("c43").unwrap()), "43");
    assert_eq!(show(it.get("s").unwrap()), "hello");
    assert_eq!(show(it.get("n").unwrap()), "14");
    let err = it.run(&format!("x = h5read(\"{path}\", \"/comp\")")).unwrap_err().to_string();
    assert!(err.contains("compound"), "{err}");
    let _ = std::fs::remove_file(path);
}

// ============================================================ MAT v7.3

fn attr_string(name: &str, value: &str) -> Vec<u8> {
    // v1 attribute message with a fixed-length scalar string
    let mut nm = name.as_bytes().to_vec();
    nm.push(0);
    let dt = dt_string(value.len() as u32);
    let sp = space_v1(&[]);
    let mut v = vec![1, 0];
    p16(&mut v, nm.len() as u16);
    p16(&mut v, dt.len() as u16);
    p16(&mut v, sp.len() as u16);
    let mut p = |mut d: Vec<u8>| {
        pad8(&mut d);
        v.extend_from_slice(&d);
    };
    p(nm);
    p(dt);
    p(sp);
    v.extend_from_slice(value.as_bytes());
    v
}

fn mat_variable(b: &mut B, dt: Vec<u8>, shape: &[u64], raw: &[u8], class: &str) -> u64 {
    let a = b.put(raw);
    b.dataset(dt, shape, layout_contig(a, raw.len() as u64), vec![(0x0c, attr_string("MATLAB_class", class))])
}

fn mat73_file() -> Vec<u8> {
    let mut b = B::new(false);
    // A = [1 2 3; 4 5 6] (MATLAB 2x3) is stored as HDF5 dims [3, 2] with the
    // column-major bytes 1,4,2,5,3,6.
    let a = mat_variable(&mut b, dt_float(8, false), &[3, 2], &f64s(&[1., 4., 2., 5., 3., 6.]), "double");
    let x = mat_variable(&mut b, dt_float(8, false), &[1, 1], &f64s(&[42.0]), "double");
    let t = mat_variable(&mut b, dt_int(2, false, false), &[2, 1], &[104, 0, 105, 0], "char");
    let sx = mat_variable(&mut b, dt_float(8, false), &[1, 1], &f64s(&[7.0]), "double");
    let sgrp = b.group(&[("f", sx)]);
    let cell = {
        let a = b.put(&[0u8; 8]);
        b.dataset(dt_compound(), &[1], layout_contig(a, 8), vec![(0x0c, attr_string("MATLAB_class", "cell"))])
    };
    let refs = b.group(&[]);
    let root = b.group(&[("#refs#", refs), ("A", a), ("c", cell), ("s", sgrp), ("t", t), ("x", x)]);
    let h5 = b.finish(root);
    let mut out = vec![0u8; 512];
    let text = b"MATLAB 7.3 MAT-file, Platform: PCWIN64, Created on: Mon Jan  1 00:00:00 2026 HDF5 schema 1.00 .";
    out[..text.len()].copy_from_slice(text);
    out.extend_from_slice(&h5);
    out
}

#[test]
fn mat_v73_is_read_through_hdf5() {
    let bytes = mat73_file();
    assert!(hdf5::is_mat_v73(&bytes));
    let vars = hdf5::read_mat_v73(&bytes).unwrap();
    let names: Vec<&str> = vars.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(names, vec!["A", "c", "s", "t", "x"], "#refs# is bookkeeping and must not appear");

    let path = tmp("v73.mat");
    std::fs::write(&path, &bytes).unwrap();
    let mut it = Interp::new();
    run(&mut it, &format!(
        r#"
m = read_mat("{path}")
r = nrow(m.A)
c = ncol(m.A)
a01 = m.A[0, 1]
a10 = m.A[1, 0]
x = m.x
t = m.t
sf = m.s.f
cell = m.c
"#));
    assert_eq!(show(it.get("r").unwrap()), "2");
    assert_eq!(show(it.get("c").unwrap()), "3");
    assert_eq!(show(it.get("a01").unwrap()), "2");
    assert_eq!(show(it.get("a10").unwrap()), "4");
    assert_eq!(show(it.get("x").unwrap()), "42");
    assert_eq!(show(it.get("t").unwrap()), "hi");
    assert_eq!(show(it.get("sf").unwrap()), "7");
    assert!(show(it.get("cell").unwrap()).contains("unsupported"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn mat_v73_truncated_is_an_error_not_a_panic() {
    let bytes = mat73_file();
    for n in (0..bytes.len()).step_by(11) {
        let _ = hdf5::read_mat_v73(&bytes[..n]);
    }
    assert!(hdf5::read_mat_v73(&bytes[..600]).is_err());
}

// ======================================================= amplification caps
//
// Each file below is small but would, without the caps, make the reader
// allocate or loop in proportion to a number the file merely *claims*.

#[test]
fn hdf5_vlen_strings_have_a_total_byte_cap() {
    // 200 elements all pointing at one 2 MiB global-heap object: 400 MiB of
    // strings from a ~2 MiB file.
    let mut b = B::new(false);
    let obj_len = 2usize << 20;
    let mut objs = Vec::new();
    p16(&mut objs, 1);
    p16(&mut objs, 1);
    p32(&mut objs, 0);
    p64(&mut objs, obj_len as u64);
    objs.extend_from_slice(&vec![b'x'; obj_len]);
    pad8(&mut objs);
    p16(&mut objs, 0);
    pad8(&mut objs);
    let mut g = b"GCOL".to_vec();
    g.extend_from_slice(&[1, 0, 0, 0]);
    p64(&mut g, (16 + objs.len()) as u64);
    g.extend_from_slice(&objs);
    let gaddr = b.put(&g);
    let mut elems = Vec::new();
    for _ in 0..200 {
        p32(&mut elems, obj_len as u32);
        p64(&mut elems, gaddr);
        p32(&mut elems, 1);
    }
    let d = b.contiguous(dt_vlen_string(), &[200], &elems);
    let root = b.group(&[("v", d)]);
    let f = b.finish(root);
    let t = std::time::Instant::now();
    let e = h5(&f, "/v").unwrap_err();
    assert!(e.contains("limit"), "{e}");
    assert!(t.elapsed().as_secs() < 5);
}

#[test]
fn hdf5_group_btree_cannot_reuse_one_symbol_node() {
    let mut b = B::new(false);
    let d = b.contiguous(dt_float(8, false), &[1], &f64s(&[1.0]));
    let mut heap = vec![0u8; 8];
    heap.extend_from_slice(b"d\0");
    pad8(&mut heap);
    let heap_data = b.put(&heap);
    let mut hh = b"HEAP".to_vec();
    hh.extend_from_slice(&[1, 0, 0, 0]);
    p64(&mut hh, heap.len() as u64);
    p64(&mut hh, 1);
    p64(&mut hh, heap_data);
    let heap_hdr = b.put(&hh);
    let mut sn = b"SNOD".to_vec();
    sn.extend_from_slice(&[1, 0]);
    p16(&mut sn, 1);
    p64(&mut sn, 8);
    p64(&mut sn, d);
    p32(&mut sn, 0);
    p32(&mut sn, 0);
    sn.extend_from_slice(&[0u8; 16]);
    let snod = b.put(&sn);
    // one leaf node whose 65535 children are all the same SNOD
    let mut bt = b"TREE".to_vec();
    bt.extend_from_slice(&[0, 0]);
    p16(&mut bt, 65535);
    p64(&mut bt, UNDEF);
    p64(&mut bt, UNDEF);
    p64(&mut bt, 0);
    for _ in 0..65535 {
        p64(&mut bt, snod);
        p64(&mut bt, 8);
    }
    let btree = b.put(&bt);
    let mut st = Vec::new();
    p64(&mut st, btree);
    p64(&mut st, heap_hdr);
    let root = b.header(&[(0x11, st)]);
    let f = b.finish(root);
    let e = hdf5::h5info(&f).unwrap_err();
    assert!(e.contains("one symbol table node"), "{e}");
}

#[test]
fn hdf5_self_referencing_continuation_block_is_rejected() {
    let mut b = B::new(false);
    pad8(&mut b.buf);
    let addr = b.buf.len() as u64;
    let mut block = Vec::new();
    for _ in 0..2 {
        p16(&mut block, 0x10);
        p16(&mut block, 16);
        block.extend_from_slice(&[0, 0, 0, 0]);
        p64(&mut block, addr);
        p64(&mut block, 48);
    }
    assert_eq!(block.len(), 48);
    b.put(&block);
    let mut cont = Vec::new();
    p64(&mut cont, addr);
    p64(&mut cont, 48);
    let d = b.header(&[(0x10, cont)]);
    let root = b.group(&[("d", d)]);
    let f = b.finish(root);
    let e = hdf5::h5info(&f).unwrap_err();
    assert!(e.contains("cycle or repeat"), "{e}");
}

#[test]
fn hdf5_chunk_index_sharing_one_address_hits_the_decoded_byte_budget() {
    // 64 MiB dataset in 512 KiB chunks; the index lists 4400 chunks (2.2 GiB
    // of decoded data) that all share a single 512 KiB block.
    let mut b = B::new(false);
    let chunk_elems = 65536usize;
    let shared = b.put(&vec![0u8; chunk_elems * 8]);
    let n_chunks = 128usize;
    let mut bt = b"TREE".to_vec();
    bt.extend_from_slice(&[1, 0]);
    p16(&mut bt, 4400);
    p64(&mut bt, UNDEF);
    p64(&mut bt, UNDEF);
    for i in 0..4400usize {
        p32(&mut bt, (chunk_elems * 8) as u32);
        p32(&mut bt, 0);
        p64(&mut bt, ((i % n_chunks) * chunk_elems) as u64);
        p64(&mut bt, 0);
        p64(&mut bt, shared);
    }
    p32(&mut bt, 0);
    p32(&mut bt, 0);
    p64(&mut bt, 0);
    p64(&mut bt, 0);
    let btree = b.put(&bt);
    let d = b.dataset(
        dt_float(8, false),
        &[(n_chunks * chunk_elems) as u64],
        layout_chunked(btree, &[chunk_elems as u32], 8),
        vec![],
    );
    let root = b.group(&[("d", d)]);
    let f = b.finish(root);
    let t = std::time::Instant::now();
    let e = h5(&f, "/d").unwrap_err();
    assert!(e.contains("repeated or overlapping"), "{e}");
    assert!(t.elapsed().as_secs() < 10);
}

#[test]
fn npz_total_size_and_entry_count_are_capped() {
    let member = npy_file(1, "<f8", false, "(1,)", &1.0f64.to_le_bytes());
    // three members each claiming ~1 GiB: individually allowed, 3 GiB total
    let one = zip_one("a.npy", &member, 8, 0, 1 << 30);
    let entry_len = 46 + "a.npy".len();
    let cd_at = one.len() - 22 - entry_len;
    let mut z = Vec::new();
    z.extend_from_slice(&one[..cd_at]);
    let entry = &one[cd_at..one.len() - 22];
    for _ in 0..3 {
        z.extend_from_slice(entry);
    }
    let cd_size = (entry.len() * 3) as u32;
    z.extend_from_slice(b"PK\x05\x06");
    z.extend_from_slice(&[0, 0, 0, 0, 3, 0, 3, 0]);
    z.extend_from_slice(&cd_size.to_le_bytes());
    z.extend_from_slice(&(cd_at as u32).to_le_bytes());
    z.extend_from_slice(&0u16.to_le_bytes());
    let e = npy::read_npz(&z).unwrap_err();
    assert!(e.contains("total limit"), "{e}");

    // 5000 entries claimed, directory big enough to hold them
    let mut z = vec![0u8; 5000 * 46];
    z.extend_from_slice(b"PK\x05\x06");
    z.extend_from_slice(&[0, 0, 0, 0]);
    z.extend_from_slice(&5000u16.to_le_bytes());
    z.extend_from_slice(&5000u16.to_le_bytes());
    z.extend_from_slice(&((5000 * 46) as u32).to_le_bytes());
    z.extend_from_slice(&0u32.to_le_bytes());
    z.extend_from_slice(&0u16.to_le_bytes());
    let e = npy::read_npz(&z).unwrap_err();
    assert!(e.contains("limit is"), "{e}");
}
