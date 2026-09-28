//! § calling C functions in a shared library (2026-09-28).
//!
//! ```text
//! lib = load_library("dsp.dll")                        # or .so / .dylib
//! y   = native_call(lib, "gain", "double(double, double)", 0.5, 2)
//! z   = lib.native_call("smooth", "void(double*, int32)", x)   # x filtered in place
//! ```
//!
//! **Fixed signatures, not arbitrary FFI.** A C function's argument types
//! are not recorded in the library, so the caller states them, and calling
//! with the wrong ones is undefined behaviour no runtime check can catch.
//! What makes that tolerable is keeping the set of signatures small enough
//! to state in one sentence and implement without a C dependency (libffi),
//! so it builds on Windows as-is:
//!
//! * **scalars** -- `R(T, T, ...)` with 0 to 6 arguments all of ONE type
//!   `T` in `double`, `int32`, `int64`, and `R` one of those or `void`;
//! * **an array kernel** -- `R(double*, N)` where `N` is `int32` or `int64`:
//!   the function gets a copy of the vector and its length; `void` returns
//!   the (possibly modified) copy, any other `R` returns the function's
//!   result;
//! * **an array transform** -- `void(double*, double*, N)`: input vector,
//!   an output buffer of the same length, the length; returns the output.
//!
//! Anything else is refused by name, with this list, before any native
//! code runs. Mixed scalar types (`double(double, int32)`) and `float` are
//! out on purpose: every one multiplies the signatures to support, and the
//! common case -- a numeric kernel someone wrote in C for speed -- is
//! covered by the three shapes above.
//!
//! **Safety.** Loading a library runs its initialisation code and calling
//! into it runs arbitrary native code: a crash there takes the interpreter
//! down with it, and no `try` can catch that. Both builtins are therefore
//! on `--sandbox`'s deny list. Libraries stay loaded until the process
//! exits -- unloading while a symbol pointer might still be in use is how
//! use-after-free happens, and a script does not load enough libraries for
//! that to matter.

use crate::{e, EvalError, ModelHandle, Value, R};
use std::sync::{Arc, Mutex};

static LIBRARIES: Mutex<Vec<Arc<libloading::Library>>> = Mutex::new(Vec::new());

pub const NAMES: &[&str] = &["load_library", "native_call"];

pub fn call(f: &str, args: &[Value]) -> R<Value> {
    match f {
        "load_library" => load(args),
        "native_call" => native_call(args),
        other => e(format!("native_ops: unknown function `{other}`")),
    }
}

fn load(args: &[Value]) -> R<Value> {
    let path = match args.first() {
        Some(Value::Str(s)) => s.clone(),
        Some(other) => return e(format!("load_library(path) needs a path string, found {}", other.type_name())),
        None => return e("load_library(path) needs the path of a .dll/.so/.dylib"),
    };
    // SAFETY: loading runs the library's initialisers. That is the whole
    // point of the builtin and is documented as unsafe at the top of this
    // module; the sandbox refuses it.
    let lib = unsafe { libloading::Library::new(&path) }
        .map_err(|err| EvalError { msg: format!("load_library: could not load `{path}`: {err}") })?;
    let mut libs = LIBRARIES.lock().unwrap();
    libs.push(Arc::new(lib));
    Ok(Value::Model(Arc::new(ModelHandle::new(
        "native_library",
        vec![
            ("path".to_string(), Value::Str(path)),
            ("id".to_string(), Value::Num((libs.len() - 1) as f64)),
        ],
    ))))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ty {
    Double,
    Int32,
    Int64,
    Void,
    DoublePtr,
}

fn parse_ty(s: &str) -> Option<Ty> {
    match s.trim() {
        "double" => Some(Ty::Double),
        "int32" | "int" => Some(Ty::Int32),
        "int64" => Some(Ty::Int64),
        "void" => Some(Ty::Void),
        "double*" | "double *" => Some(Ty::DoublePtr),
        _ => None,
    }
}

const SHAPES: &str = "supported signatures: R(T, ...) with 0-6 arguments all of one type T in \
     double/int32/int64 and R in double/int32/int64/void; R(double*, N); \
     void(double*, double*, N) -- N is int32 or int64";

/// `"double(double, double)"` -> `(Double, [Double, Double])`.
fn parse_sig(sig: &str) -> R<(Ty, Vec<Ty>)> {
    let bad = || EvalError { msg: format!("native_call: cannot read the signature `{sig}` -- {SHAPES}") };
    let open = sig.find('(').ok_or_else(bad)?;
    let body = sig[open + 1..].strip_suffix(')').ok_or_else(bad)?;
    let ret = parse_ty(&sig[..open]).ok_or_else(bad)?;
    if ret == Ty::DoublePtr {
        return Err(bad());
    }
    let args: Vec<Ty> = if body.trim().is_empty() || body.trim() == "void" {
        Vec::new()
    } else {
        body.split(',').map(|a| parse_ty(a).ok_or_else(bad)).collect::<R<_>>()?
    };
    Ok((ret, args))
}

enum Shape {
    Scalars(Ty),
    Kernel(Ty),
    Transform(Ty),
}

fn shape_of(sig: &str, ret: Ty, args: &[Ty]) -> R<Shape> {
    let refuse = |why: &str| e(format!("native_call: `{sig}` {why} -- {SHAPES}"));
    let len_ty = |t: Ty| matches!(t, Ty::Int32 | Ty::Int64);
    match args {
        [Ty::DoublePtr, n] if len_ty(*n) => Ok(Shape::Kernel(*n)),
        [Ty::DoublePtr, Ty::DoublePtr, n] if len_ty(*n) => {
            if ret != Ty::Void {
                return refuse("is an array transform, which must return void");
            }
            Ok(Shape::Transform(*n))
        }
        _ if args.contains(&Ty::DoublePtr) => refuse("uses double* in an unsupported position"),
        _ if args.contains(&Ty::Void) => refuse("has a void argument"),
        _ if args.len() > 6 => refuse("has more than 6 arguments"),
        [] => Ok(Shape::Scalars(Ty::Double)),
        [first, rest @ ..] => {
            if rest.iter().any(|t| t != first) {
                return refuse("mixes argument types");
            }
            Ok(Shape::Scalars(*first))
        }
    }
}

fn native_call(args: &[Value]) -> R<Value> {
    let lib_id = match args.first() {
        Some(Value::Model(m)) if m.kind == "native_library" => match m.field("id") {
            Some(Value::Num(n)) => *n as usize,
            _ => return e("native_call: this library handle is damaged"),
        },
        Some(other) => {
            return e(format!("native_call(lib, name, signature, ...) needs a load_library handle first, found {}", other.type_name()))
        }
        None => return e("native_call(lib, name, signature, ...) needs a load_library handle"),
    };
    let name = match args.get(1) {
        Some(Value::Str(s)) => s.clone(),
        _ => return e("native_call: the second argument is the function's name, as a string"),
    };
    let sig = match args.get(2) {
        Some(Value::Str(s)) => s.clone(),
        _ => return e(format!("native_call: the third argument is `{name}`'s C signature, e.g. \"double(double)\"")),
    };
    let (ret, arg_tys) = parse_sig(&sig)?;
    let shape = shape_of(&sig, ret, &arg_tys)?;
    let values = &args[3..];
    let lib = LIBRARIES
        .lock()
        .unwrap()
        .get(lib_id)
        .cloned()
        .ok_or_else(|| EvalError { msg: "native_call: unknown library handle".into() })?;
    // SAFETY: the symbol is only called through the signature the script
    // declared; see the module doc for why that is the contract.
    let ptr: *const () = unsafe {
        let sym: libloading::Symbol<unsafe extern "C" fn()> = lib
            .get(name.as_bytes())
            .map_err(|err| EvalError { msg: format!("native_call: no function `{name}` in the library: {err}") })?;
        *sym as *const ()
    };
    unsafe { call_ptr(ptr, &sig, ret, shape, arg_tys.len(), values) }
}

/// Call `ptr` as `sig`. Separate from the symbol lookup so the calling
/// machinery can be tested against Rust `extern "C"` functions directly.
unsafe fn call_ptr(ptr: *const (), sig: &str, ret: Ty, shape: Shape, n_args: usize, values: &[Value]) -> R<Value> {
    match shape {
        Shape::Scalars(t) => {
            if values.len() != n_args {
                return e(format!("native_call: `{sig}` takes {n_args} argument(s), got {}", values.len()));
            }
            let nums: Vec<f64> = values
                .iter()
                .enumerate()
                .map(|(i, v)| match v {
                    Value::Num(x) => Ok(*x),
                    Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
                    other => e(format!("native_call: argument {} is {}, expected a number", i + 1, other.type_name())),
                })
                .collect::<R<_>>()?;
            match t {
                Ty::Double => Ok(scalars::<f64>(ptr, &nums, ret)),
                Ty::Int32 => Ok(scalars::<i32>(ptr, &to_ints(&nums, sig, i32::MIN as f64, i32::MAX as f64)?.iter().map(|&v| v as i32).collect::<Vec<_>>(), ret)),
                Ty::Int64 => Ok(scalars::<i64>(ptr, &to_ints(&nums, sig, -(2f64.powi(53)), 2f64.powi(53))?, ret)),
                _ => unreachable!("shape_of admits only double/int32/int64 scalars"),
            }
        }
        Shape::Kernel(n) | Shape::Transform(n) => {
            let is_transform = matches!(shape, Shape::Transform(_));
            let [x] = values else {
                return e(format!("native_call: `{sig}` takes one vector (its length is passed for you), got {} argument(s)", values.len()));
            };
            let mut buf = crate::to_vec(x)?;
            let len = buf.len();
            if n == Ty::Int32 && len > i32::MAX as usize {
                return e(format!("native_call: a vector of {len} elements does not fit an int32 length"));
            }
            if is_transform {
                let mut out = vec![0.0; len];
                match n {
                    Ty::Int32 => std::mem::transmute::<*const (), unsafe extern "C" fn(*const f64, *mut f64, i32)>(ptr)(buf.as_ptr(), out.as_mut_ptr(), len as i32),
                    _ => std::mem::transmute::<*const (), unsafe extern "C" fn(*const f64, *mut f64, i64)>(ptr)(buf.as_ptr(), out.as_mut_ptr(), len as i64),
                }
                return Ok(Value::Vec(Arc::new(out)));
            }
            macro_rules! kernel {
                ($r:ty) => {
                    match n {
                        Ty::Int32 => std::mem::transmute::<*const (), unsafe extern "C" fn(*mut f64, i32) -> $r>(ptr)(buf.as_mut_ptr(), len as i32),
                        _ => std::mem::transmute::<*const (), unsafe extern "C" fn(*mut f64, i64) -> $r>(ptr)(buf.as_mut_ptr(), len as i64),
                    }
                };
            }
            Ok(match ret {
                Ty::Void => {
                    kernel!(());
                    Value::Vec(Arc::new(buf))
                }
                Ty::Double => Value::Num(kernel!(f64)),
                Ty::Int32 => Value::Num(kernel!(i32) as f64),
                _ => Value::Num(kernel!(i64) as f64),
            })
        }
    }
}

fn to_ints(nums: &[f64], sig: &str, lo: f64, hi: f64) -> R<Vec<i64>> {
    nums.iter()
        .enumerate()
        .map(|(i, &v)| {
            if v.fract() != 0.0 || !(lo..=hi).contains(&v) {
                e(format!("native_call: argument {} is {v}, which is not an integer `{sig}` can take", i + 1))
            } else {
                Ok(v as i64)
            }
        })
        .collect()
}

/// Call a scalar function of 0-6 arguments of type `T`, returning `ret`.
unsafe fn scalars<T: Copy>(ptr: *const (), a: &[T], ret: Ty) -> Value {
    macro_rules! arity {
        ($r:ty) => {
            match a.len() {
                0 => std::mem::transmute::<*const (), unsafe extern "C" fn() -> $r>(ptr)(),
                1 => std::mem::transmute::<*const (), unsafe extern "C" fn(T) -> $r>(ptr)(a[0]),
                2 => std::mem::transmute::<*const (), unsafe extern "C" fn(T, T) -> $r>(ptr)(a[0], a[1]),
                3 => std::mem::transmute::<*const (), unsafe extern "C" fn(T, T, T) -> $r>(ptr)(a[0], a[1], a[2]),
                4 => std::mem::transmute::<*const (), unsafe extern "C" fn(T, T, T, T) -> $r>(ptr)(a[0], a[1], a[2], a[3]),
                5 => std::mem::transmute::<*const (), unsafe extern "C" fn(T, T, T, T, T) -> $r>(ptr)(a[0], a[1], a[2], a[3], a[4]),
                _ => std::mem::transmute::<*const (), unsafe extern "C" fn(T, T, T, T, T, T) -> $r>(ptr)(a[0], a[1], a[2], a[3], a[4], a[5]),
            }
        };
    }
    match ret {
        Ty::Double => Value::Num(arity!(f64)),
        Ty::Int32 => Value::Num(arity!(i32) as f64),
        Ty::Int64 => Value::Num(arity!(i64) as f64),
        _ => {
            arity!(());
            Value::Nothing
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn add3(a: f64, b: f64, c: f64) -> f64 {
        a + b + c
    }
    extern "C" fn imul(a: i32, b: i32) -> i32 {
        a * b
    }
    extern "C" fn big(a: i64) -> i64 {
        a * 2
    }
    extern "C" fn scale(buf: *mut f64, n: i32) {
        let s = unsafe { std::slice::from_raw_parts_mut(buf, n as usize) };
        s.iter_mut().for_each(|v| *v *= 10.0);
    }
    extern "C" fn total(buf: *mut f64, n: i64) -> f64 {
        unsafe { std::slice::from_raw_parts(buf, n as usize) }.iter().sum()
    }
    extern "C" fn diff(inp: *const f64, out: *mut f64, n: i64) {
        let (i, o) = unsafe { (std::slice::from_raw_parts(inp, n as usize), std::slice::from_raw_parts_mut(out, n as usize)) };
        for k in 0..n as usize {
            o[k] = if k == 0 { 0.0 } else { i[k] - i[k - 1] };
        }
    }

    fn go(ptr: *const (), sig: &str, vals: Vec<Value>) -> R<Value> {
        let (ret, tys) = parse_sig(sig)?;
        let shape = shape_of(sig, ret, &tys)?;
        unsafe { call_ptr(ptr, sig, ret, shape, tys.len(), &vals) }
    }

    fn v(xs: &[f64]) -> Value {
        Value::Vec(Arc::new(xs.to_vec()))
    }

    #[test]
    fn every_supported_shape_calls_through() {
        let n = |x: f64| Value::Num(x);
        assert!(matches!(go(add3 as *const (), "double(double, double, double)", vec![n(1.0), n(2.0), n(3.5)]), Ok(Value::Num(x)) if x == 6.5));
        assert!(matches!(go(imul as *const (), "int32(int32,int32)", vec![n(6.0), n(-7.0)]), Ok(Value::Num(x)) if x == -42.0));
        assert!(matches!(go(big as *const (), "int64(int64)", vec![n(4e15)]), Ok(Value::Num(x)) if x == 8e15));
        match go(scale as *const (), "void(double*, int32)", vec![v(&[1.0, 2.0])]).unwrap() {
            Value::Vec(out) => assert_eq!(*out, vec![10.0, 20.0]),
            other => panic!("{other:?}"),
        }
        assert!(matches!(go(total as *const (), "double(double*, int64)", vec![v(&[1.0, 2.0, 4.0])]), Ok(Value::Num(x)) if x == 7.0));
        match go(diff as *const (), "void(double*, double*, int64)", vec![v(&[1.0, 4.0, 9.0])]).unwrap() {
            Value::Vec(out) => assert_eq!(*out, vec![0.0, 3.0, 5.0]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn unsupported_signatures_are_refused_before_any_call() {
        for sig in ["double(double, int32)", "float(float)", "double(double*)", "int32(double*, double*, int64)", "double", "double(x)"] {
            let (ok_parse, _) = match parse_sig(sig) {
                Ok((r, a)) => (shape_of(sig, r, &a).is_ok(), ()),
                Err(_) => (false, ()),
            };
            assert!(!ok_parse, "`{sig}` should be refused");
        }
    }

    #[test]
    fn argument_count_and_integer_checks() {
        let n = |x: f64| Value::Num(x);
        assert!(go(add3 as *const (), "double(double, double, double)", vec![n(1.0)]).unwrap_err().msg.contains("takes 3"));
        assert!(go(imul as *const (), "int32(int32, int32)", vec![n(1.5), n(2.0)]).unwrap_err().msg.contains("not an integer"));
    }
}
