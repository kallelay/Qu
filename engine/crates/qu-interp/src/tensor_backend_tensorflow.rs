//! `TensorFlowBackend` -- third `TensorBackend` implementation, on top of the
//! official `tensorflow` crate (Rust bindings to libtensorflow's C API),
//! gated `#[cfg(feature = "backend-tensorflow")]` (2026-08-26).
//!
//! Kept in its OWN file (declared from `lib.rs`, see that `mod` item's own
//! doc comment) rather than inside `tensor_backend.rs` -- that file was
//! under live, unrelated, concurrent extension (`conv2d`/`gru_cell`/
//! attention/etc., adding eleven more required `TensorBackend` methods)
//! while this was written, so touching it directly risked either colliding
//! with that work or accidentally bundling it into this task's commit; a
//! separate file implementing the SAME `pub trait TensorBackend` from
//! `crate::tensor_backend` sidesteps that entirely. See IMPL.md's dated
//! `backend-tensorflow` entry for the full "shared-tree collision" account.
//!
//! **Why this needs a different shape than `TorchBackend`.** `tch` gives
//! each `Tensor` its own attached autograd graph (`requires_grad` +
//! `run_backward`), so `TorchBackend` (in `tensor_backend.rs`) can be a thin
//! one-call-per-method wrapper: every `TensorBackend` method just calls
//! straight into libtorch's OWN eager op and returns a concrete result, and
//! by the time `backward_and_grads` runs, the graph connecting `loss` back
//! to `params` already lives INSIDE the `loss` tensor's C++ object --
//! nothing else to track.
//!
//! libtensorflow's C API has no such eager-tensor-carries-its-own-graph
//! concept. Its autodiff entry point, `TF_AddGradients` (exposed here as
//! `Graph::add_gradients`, the same call `tensorflow::train::Optimizer`'s
//! own `compute_gradients` uses internally -- see that crate's own
//! `src/train.rs`), differentiates a STATIC `Graph` of symbolic `Output`
//! nodes; it has nothing to work from if all it's handed is two bare
//! concrete numbers (`loss`, `params`) with no record of how one was
//! computed from the other. That symbolic graph has to exist SOMEWHERE by
//! the time `backward_and_grads` is called -- but this trait's `dense`/
//! `relu`/`mse` methods only get single tensor handles in and one out,
//! never the surrounding `Scope`, so there is no way for them to hand back
//! a graph alongside their result.
//!
//! The fix used here: `TensorFlowBackend` keeps a small internal
//! `tape: Vec<TapeEntry>` -- NOT libtensorflow API, just a plain Rust `Vec`
//! recording, in order, every `param`/`dense`/`relu`/`mse` call this epoch
//! and what it was built from (mirroring, on a tiny scale, exactly what
//! `NativeBackend`'s own tape in `lib.rs` already does for the SAME
//! reason). `dense`/`relu`/`mse` compute their result with plain host `f32`
//! arithmetic (the actual forward math -- matmul+bias, `max(x,0)`, mean
//! squared error -- is simple enough to hand-write correctly, and doing so
//! here means every OTHER method stays a trivial, allocation-only "record +
//! compute" step, no graph/session cost per call). `backward_and_grads` is
//! the one method that pays for all of this: it replays the recorded tape
//! as a FRESH `Scope` (a `Placeholder` per leaf, an `ops::constant` per
//! plain-data operand, `ops::mat_mul`/`add`/`relu`/`sub`/`mul`/`mean`
//! composed in recorded order), calls the REAL `Graph::add_gradients` on
//! it, runs ONE `Session`, and reads the gradients back -- so the actual
//! differentiation is genuinely libtensorflow's own reverse-mode engine,
//! not a hand-rolled backprop; only the forward numbers and the bookkeeping
//! to reach that one call are home-grown. The tape is cleared at the end of
//! every `backward_and_grads` call, so it never grows past one epoch's
//! worth of ops. See IMPL.md's dated entry for the fuller investigation
//! notes (what was tried before landing on this shape, and exactly what
//! about the `tensorflow` crate's own API forced it).
//!
//! **Scope**: matches `TorchBackend`'s own FIRST slice exactly --
//! `dense`+`relu`+`mse`+SGD only (`train_dense_mlp`, unchanged, same
//! generic function every `TensorBackend` runs through), per this task's
//! own brief ("match the scope the torch backend proved FIRST, before that
//! one was extended further"). `TensorBackend` itself grew eleven more
//! methods (`elementwise_mul`/`add`/`matmul`/`sum_all`/`scale`/`conv2d`/
//! `maxpool2d`/`gru_cell`/`scaled_dot_product_attention`/
//! `row_of_gru_output`/`transpose`, plus three more generic training loops)
//! in that concurrent session's work; this backend implements those eleven
//! as honest stubs that return a clear `Err` (never silently wrong math)
//! rather than real coverage -- see the "Extended-trait stubs" comment
//! below. Measured ~150x slower than `TorchBackend` even on the SIMPLEST
//! op sequence (see IMPL.md) -- strong evidence extending this design
//! further, rather than first fixing the `Session`-per-epoch overhead that
//! causes it, would not be a good use of effort. Filling these in for real
//! is tracked in `BACKLOG.md`.

use crate::tensor_backend::{GruWeights, TensorBackend};
use crate::EvalError;
use std::collections::HashMap;
use tensorflow::{
    ops, DataType, Operation, Output, Scope, Session, SessionOptions, SessionRunArgs,
    Tensor as TfTensor,
};

type R<T> = Result<T, EvalError>;

/// One value flowing through [`TensorFlowBackend`]: always a concrete
/// row-major `(rows, cols)` `f32` array (so every method can return a real
/// number immediately, matching this trait's eager-per-call contract),
/// plus -- if this value was produced by `param`/`dense`/`relu`/`mse` this
/// epoch -- the index of its entry in `self.tape` (see this module's own
/// doc comment above for why that index is needed at all).
#[derive(Clone)]
pub struct TfT {
    rows: usize,
    cols: usize,
    data: Vec<f32>,
    tape_id: Option<usize>,
}

/// One operand of a recorded tape op: either a reference to an earlier tape
/// entry (an intermediate result, still part of THIS epoch's differentiable
/// chain), or plain untaped data (e.g. `x_t`/`y_t` from `from_rows`, which
/// `train_dense_mlp` never wraps in `param()` and so never needs a gradient
/// of their own -- replayed as an `ops::constant` instead of a
/// `Placeholder`).
#[derive(Clone)]
enum Operand {
    Tape(usize),
    Data { rows: usize, cols: usize, data: Vec<f32> },
}

fn operand_of(t: &TfT) -> Operand {
    match t.tape_id {
        Some(id) => Operand::Tape(id),
        None => Operand::Data { rows: t.rows, cols: t.cols, data: t.data.clone() },
    }
}

#[derive(Clone)]
enum TapeKind {
    /// A fresh trainable leaf (from `param()`) -- replayed as a
    /// `Placeholder` fed with this entry's own `data`.
    Leaf,
    Dense { x: Operand, w: Operand, b: Operand },
    Relu { x: Operand },
    Mse { pred: Operand, y: Operand },
}

#[derive(Clone)]
struct TapeEntry {
    rows: usize,
    cols: usize,
    data: Vec<f32>,
    kind: TapeKind,
}

fn tf_err(context: &str, err: impl std::fmt::Display) -> EvalError {
    EvalError { msg: format!("TensorFlowBackend: {context}: {err}") }
}

fn tf_unimplemented(op: &str) -> EvalError {
    EvalError { msg: format!(
        "TensorFlowBackend: `{op}` is not implemented -- this backend covers only the \
         original dense/relu/mse/sgd vertical slice (see IMPL.md's dated \
         `backend-tensorflow` entry for why). Use engine=\"native\" or engine=\"torch\" \
         for conv2d/GRU/attention/elementwise ops."
    ) }
}

/// Real-libtensorflow implementation of [`TensorBackend`] via the
/// `tensorflow` crate. Always `f32` (matching `TorchBackend`'s own choice,
/// for the same "the alternate-engine path trades precision for using the
/// vendor's own kernels" reason), always CPU (the prebuilt libtensorflow
/// this links against, per `Cargo.toml`'s own comment, is the CPU-only
/// build).
///
/// See this module's own doc comment (top of file) for what `tape` is and
/// why it exists.
#[derive(Default)]
pub struct TensorFlowBackend {
    tape: Vec<TapeEntry>,
}

impl TensorFlowBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Builds (or looks up, for an already-built tape entry) the graph `Output`
/// for one operand, inside `backward_and_grads`'s replay. `built[i]` is
/// filled in strictly left-to-right as tape entries are replayed, so any
/// `Operand::Tape(i)` reference here is always to an EARLIER, already-built
/// index (tape entries can only reference entries recorded before them).
fn resolve_operand(op: &Operand, built: &[Option<Output>], scope: &mut Scope) -> R<Output> {
    match op {
        Operand::Tape(i) => built
            .get(*i)
            .and_then(|o| o.clone())
            .ok_or_else(|| EvalError { msg: format!("TensorFlowBackend: tape entry {i} referenced before it was built (internal bug)") }),
        Operand::Data { rows, cols, data } => {
            let t = TfTensor::<f32>::new(&[*rows as u64, *cols as u64])
                .with_values(data)
                .map_err(|e| tf_err("building a constant tensor", e))?;
            let op = ops::constant(t, scope).map_err(|e| tf_err("Const op", e))?;
            Ok(op.into())
        }
    }
}

impl TensorBackend for TensorFlowBackend {
    type T = TfT;

    fn from_rows(&mut self, rows: usize, cols: usize, data: Vec<f64>) -> R<TfT> {
        if data.len() != rows * cols {
            return Err(EvalError { msg: format!("TensorFlowBackend: expected {} elements for a {rows}x{cols} tensor, got {}", rows * cols, data.len()) });
        }
        Ok(TfT { rows, cols, data: data.iter().map(|&v| v as f32).collect(), tape_id: None })
    }

    fn to_rows(&mut self, t: &TfT) -> R<(usize, usize, Vec<f64>)> {
        Ok((t.rows, t.cols, t.data.iter().map(|&v| v as f64).collect()))
    }

    fn param(&mut self, t: TfT) -> R<TfT> {
        // A fresh leaf every step, matching `TorchBackend::param`'s own
        // "detach from whatever graph `t` came from, start tracking fresh"
        // semantics -- the previous step's `sgd_update` result is always
        // untaped (`tape_id: None`) anyway, so there is nothing to actually
        // detach from here, just a new tape entry to open.
        let idx = self.tape.len();
        self.tape.push(TapeEntry { rows: t.rows, cols: t.cols, data: t.data.clone(), kind: TapeKind::Leaf });
        Ok(TfT { rows: t.rows, cols: t.cols, data: t.data, tape_id: Some(idx) })
    }

    fn dense(&mut self, x: &TfT, w: &TfT, b: &TfT) -> R<TfT> {
        if x.cols != w.rows {
            return Err(EvalError { msg: format!("TensorFlowBackend dense: x is {}x{} but w is {}x{} (x.cols must equal w.rows)", x.rows, x.cols, w.rows, w.cols) });
        }
        if b.rows != 1 || b.cols != w.cols {
            return Err(EvalError { msg: format!("TensorFlowBackend dense: b is {}x{}, expected 1x{}", b.rows, b.cols, w.cols) });
        }
        let mut data = vec![0f32; x.rows * w.cols];
        for r in 0..x.rows {
            for c in 0..w.cols {
                let mut sum = 0f32;
                for k in 0..x.cols {
                    sum += x.data[r * x.cols + k] * w.data[k * w.cols + c];
                }
                data[r * w.cols + c] = sum + b.data[c];
            }
        }
        let idx = self.tape.len();
        self.tape.push(TapeEntry {
            rows: x.rows,
            cols: w.cols,
            data: data.clone(),
            kind: TapeKind::Dense { x: operand_of(x), w: operand_of(w), b: operand_of(b) },
        });
        Ok(TfT { rows: x.rows, cols: w.cols, data, tape_id: Some(idx) })
    }

    fn relu(&mut self, x: &TfT) -> R<TfT> {
        let data: Vec<f32> = x.data.iter().map(|&v| v.max(0.0)).collect();
        let idx = self.tape.len();
        self.tape.push(TapeEntry { rows: x.rows, cols: x.cols, data: data.clone(), kind: TapeKind::Relu { x: operand_of(x) } });
        Ok(TfT { rows: x.rows, cols: x.cols, data, tape_id: Some(idx) })
    }

    fn mse(&mut self, pred: &TfT, y: &TfT) -> R<TfT> {
        if pred.rows != y.rows || pred.cols != y.cols {
            return Err(EvalError { msg: format!("TensorFlowBackend mse: pred is {}x{} but y is {}x{}", pred.rows, pred.cols, y.rows, y.cols) });
        }
        let n = pred.data.len() as f32;
        let sum: f32 = pred.data.iter().zip(&y.data).map(|(p, yv)| (p - yv) * (p - yv)).sum();
        let mean = sum / n;
        let idx = self.tape.len();
        self.tape.push(TapeEntry { rows: 1, cols: 1, data: vec![mean], kind: TapeKind::Mse { pred: operand_of(pred), y: operand_of(y) } });
        Ok(TfT { rows: 1, cols: 1, data: vec![mean], tape_id: Some(idx) })
    }

    fn backward_and_grads(&mut self, loss: &TfT, params: &[TfT]) -> R<Vec<TfT>> {
        let loss_idx = loss.tape_id.ok_or_else(|| EvalError { msg: "TensorFlowBackend: loss has no recorded tape entry -- it must come from this backend's own mse()".into() })?;

        let mut scope = Scope::new_root_scope();
        let scope = &mut scope;
        let mut built: Vec<Option<Output>> = vec![None; self.tape.len()];
        // Per leaf tape index: the Placeholder op plus the concrete tensor
        // it must be fed at run time.
        let mut leaf_feeds: HashMap<usize, (Operation, TfTensor<f32>)> = HashMap::new();

        for i in 0..self.tape.len() {
            let entry = self.tape[i].clone();
            let out: Output = match &entry.kind {
                TapeKind::Leaf => {
                    let ph = ops::Placeholder::new()
                        .dtype(DataType::Float)
                        .shape([entry.rows as u64, entry.cols as u64])
                        .build(&mut scope.with_op_name(&format!("leaf_{i}")))
                        .map_err(|e| tf_err("building a leaf placeholder", e))?;
                    let feed = TfTensor::<f32>::new(&[entry.rows as u64, entry.cols as u64])
                        .with_values(&entry.data)
                        .map_err(|e| tf_err("building a leaf feed tensor", e))?;
                    leaf_feeds.insert(i, (ph.clone(), feed));
                    ph.into()
                }
                TapeKind::Dense { x, w, b } => {
                    let xo = resolve_operand(x, &built, scope)?;
                    let wo = resolve_operand(w, &built, scope)?;
                    let bo = resolve_operand(b, &built, scope)?;
                    let mm = ops::mat_mul(xo, wo, scope).map_err(|e| tf_err("MatMul op", e))?;
                    ops::add(mm, bo, scope).map_err(|e| tf_err("Add op", e))?.into()
                }
                TapeKind::Relu { x } => {
                    let xo = resolve_operand(x, &built, scope)?;
                    ops::relu(xo, scope).map_err(|e| tf_err("Relu op", e))?.into()
                }
                TapeKind::Mse { pred, y } => {
                    let po = resolve_operand(pred, &built, scope)?;
                    let yo = resolve_operand(y, &built, scope)?;
                    let diff = ops::sub(po, yo, scope).map_err(|e| tf_err("Sub op", e))?;
                    let sq = ops::mul(diff.clone(), diff, scope).map_err(|e| tf_err("Mul op", e))?;
                    let axes = ops::constant(&[0i32, 1i32][..], scope).map_err(|e| tf_err("axes Const op", e))?;
                    ops::mean(sq, axes, scope).map_err(|e| tf_err("Mean op", e))?.into()
                }
            };
            built[i] = Some(out);
        }

        let loss_output = built[loss_idx].clone().ok_or_else(|| EvalError { msg: "TensorFlowBackend: internal error building the replayed graph (loss node missing)".into() })?;
        let mut param_outputs: Vec<Output> = Vec::with_capacity(params.len());
        for p in params {
            let id = p.tape_id.ok_or_else(|| EvalError { msg: "TensorFlowBackend: a param passed to backward_and_grads has no tape entry -- it must come from this backend's own param()".into() })?;
            let out = built.get(id).and_then(|o| o.clone()).ok_or_else(|| EvalError { msg: "TensorFlowBackend: internal error resolving a param's tape entry".into() })?;
            param_outputs.push(out);
        }

        let grad_outputs = scope
            .graph_mut()
            .add_gradients(None, &[loss_output], &param_outputs, None)
            .map_err(|e| tf_err("add_gradients (TF_AddGradients)", e))?;

        let session = Session::new(&SessionOptions::new(), &scope.graph())
            .map_err(|e| tf_err("creating a Session", e))?;
        let mut run_args = SessionRunArgs::new();
        for (ph, feed) in leaf_feeds.values() {
            run_args.add_feed(ph, 0, feed);
        }
        let mut fetch_tokens = Vec::with_capacity(grad_outputs.len());
        for (i, g) in grad_outputs.iter().enumerate() {
            let g = g.as_ref().ok_or_else(|| EvalError { msg: format!("TensorFlowBackend: add_gradients returned no gradient for param {i} (it did not reach the loss)") })?;
            fetch_tokens.push(run_args.request_fetch(&g.operation, g.index));
        }
        session.run(&mut run_args).map_err(|e| tf_err("Session::run", e))?;

        let mut grads = Vec::with_capacity(params.len());
        for (i, token) in fetch_tokens.into_iter().enumerate() {
            let fetched = run_args.fetch::<f32>(token).map_err(|e| tf_err("fetching a gradient", e))?;
            let (rows, cols) = (params[i].rows, params[i].cols);
            if fetched.len() != rows * cols {
                return Err(EvalError { msg: format!("TensorFlowBackend: gradient {i} came back with {} elements, expected {}x{}={}", fetched.len(), rows, cols, rows * cols) });
            }
            grads.push(TfT { rows, cols, data: fetched.to_vec(), tape_id: None });
        }

        self.tape.clear();
        Ok(grads)
    }

    fn sgd_update(&mut self, param: &TfT, grad: &TfT, lr: f64) -> R<TfT> {
        if param.rows != grad.rows || param.cols != grad.cols {
            return Err(EvalError { msg: format!("TensorFlowBackend sgd_update: param is {}x{} but grad is {}x{}", param.rows, param.cols, grad.rows, grad.cols) });
        }
        let lr = lr as f32;
        let data: Vec<f32> = param.data.iter().zip(&grad.data).map(|(p, g)| p - lr * g).collect();
        Ok(TfT { rows: param.rows, cols: param.cols, data, tape_id: None })
    }

    fn scalar(&mut self, t: &TfT) -> R<f64> {
        if t.data.len() != 1 {
            return Err(EvalError { msg: format!("TensorFlowBackend: scalar() called on a {}x{} tensor ({} elements, expected 1)", t.rows, t.cols, t.data.len()) });
        }
        Ok(t.data[0] as f64)
    }

    // ---- Extended-trait stubs (2026-08-26) -- see this file's own doc
    // comment (top) for why these are honest `Err`s rather than real
    // implementations: `TensorFlowBackend` was scoped to the ORIGINAL
    // dense/relu/mse/sgd vertical slice only, per this task's brief, and
    // these eleven methods were added to `TensorBackend` by a concurrent,
    // unrelated session while this task ran. Returning a clear error here
    // (never silently wrong numbers) is what keeps `--features
    // backend-tensorflow` compiling against the current trait without
    // pretending to cover ground this task didn't actually implement or
    // verify.

    fn elementwise_mul(&mut self, _a: &TfT, _b: &TfT) -> R<TfT> {
        Err(tf_unimplemented("elementwise_mul"))
    }

    fn add(&mut self, _a: &TfT, _b: &TfT) -> R<TfT> {
        Err(tf_unimplemented("add"))
    }

    fn matmul(&mut self, _a: &TfT, _b: &TfT) -> R<TfT> {
        Err(tf_unimplemented("matmul"))
    }

    fn sum_all(&mut self, _t: &TfT) -> R<TfT> {
        Err(tf_unimplemented("sum_all"))
    }

    fn scale(&mut self, _t: &TfT, _s: f64) -> R<TfT> {
        Err(tf_unimplemented("scale"))
    }

    fn conv2d(&mut self, _x: &TfT, _kernel: &TfT, _stride: usize, _padding: &str) -> R<TfT> {
        Err(tf_unimplemented("conv2d"))
    }

    fn maxpool2d(&mut self, _x: &TfT, _pool: usize, _stride: usize) -> R<TfT> {
        Err(tf_unimplemented("maxpool2d"))
    }

    fn gru_cell(&mut self, _x: &TfT, _h: &TfT, _w: &GruWeights<TfT>) -> R<TfT> {
        Err(tf_unimplemented("gru_cell"))
    }

    fn scaled_dot_product_attention(&mut self, _q: &TfT, _k: &TfT, _v: &TfT) -> R<TfT> {
        Err(tf_unimplemented("scaled_dot_product_attention"))
    }

    fn row_of_gru_output(&mut self, _t: &TfT) -> R<TfT> {
        Err(tf_unimplemented("row_of_gru_output"))
    }

    fn transpose(&mut self, _t: &TfT) -> R<TfT> {
        Err(tf_unimplemented("transpose"))
    }

    fn softmax_cross_entropy(&mut self, _logits: &[TfT], _target: usize) -> R<TfT> {
        Err(tf_unimplemented("softmax_cross_entropy"))
    }
}

#[cfg(test)]
mod tensorflow_tests {
    use super::*;
    use crate::tensor_backend::{train_dense_mlp, DenseLayerShape};

    /// Same XOR smoke test as `tensor_backend::torch_tests::torch_backend_trains_xor_down_to_a_low_loss`,
    /// same fixed init, same architecture -- proves `TensorFlowBackend`'s
    /// tape-replay + `add_gradients` path differentiates dense+relu+dense+mse
    /// correctly enough to actually learn a non-linearly-separable function,
    /// not just run without erroring.
    #[test]
    fn tensorflow_backend_trains_xor_down_to_a_low_loss() {
        let shapes = [
            DenseLayerShape { in_dim: 2, out_dim: 8, relu: true },
            DenseLayerShape { in_dim: 8, out_dim: 1, relu: false },
        ];
        let w0 = vec![
            0.5, -0.3, 0.2, 0.4, -0.5, 0.1, -0.2, 0.3,
            -0.4, 0.6, -0.1, 0.5, 0.3, -0.6, 0.2, -0.4,
        ];
        let b0 = vec![0.0; 8];
        let w1 = vec![0.3, -0.4, 0.5, -0.2, 0.4, -0.3, 0.2, -0.5];
        let b1 = vec![0.0];
        let x = vec![0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0];
        let y = vec![0.0, 1.0, 1.0, 0.0];

        let mut backend = TensorFlowBackend::new();
        let result = train_dense_mlp(
            &mut backend, &shapes, &[w0, w1], &[b0, b1],
            4, 2, x, 4, 1, y, 2000, 0.5,
        ).unwrap();

        let first = result.loss_history[0];
        let last = *result.loss_history.last().unwrap();
        assert!(last < first, "loss should decrease: first={first}, last={last}");
        assert!(last < 0.05, "XOR should converge to a low MSE with 2000 SGD steps, got {last}");
    }

    #[test]
    fn from_rows_to_rows_round_trips() {
        let mut backend = TensorFlowBackend::new();
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let t = backend.from_rows(2, 3, data.clone()).unwrap();
        let (r, c, out) = backend.to_rows(&t).unwrap();
        assert_eq!((r, c), (2, 3));
        assert_eq!(out, data);
    }

    /// Numerically cross-checks `TensorFlowBackend`'s forward pass AND
    /// gradients against `NativeBackend`'s own tape (via `Interp::eval_grad`,
    /// exercised through the ordinary `dense`/`relu`/`mse` builtins) on the
    /// same small input -- the same rigor `backend-torch`'s own verification
    /// used before it had a dedicated Qu-level test. Kept a plain Rust unit
    /// test (not a `.qu` script) since it needs to reach `NativeBackend`
    /// directly for a value-for-value gradient comparison, not just a
    /// converged-loss smoke check.
    ///
    /// `b0`/`b1` are chosen so no `dense0` pre-activation lands near zero
    /// (deliberately NOT reusing the XOR test's own `b0=[0.1,-0.1,0.05]`,
    /// which puts `dense0`'s first row within 1e-17 of zero after
    /// cancellation -- e.g. `1*0.2 + -1*0.3 + 0.1` -- so `f64`
    /// (`NativeBackend`) and `f32` (`TensorFlowBackend`) round to opposite
    /// sides of ReLU's `x > 0` boundary there and flip that unit's mask,
    /// which is a genuine floating-point knife-edge, not a bug in either
    /// backend's math -- confirmed by hand-deriving both backends' own
    /// documented vjp rules against that input and getting `TensorFlowBackend`'s
    /// answer, not `NativeBackend`'s actual (`f64`-rounded) one).
    #[test]
    fn tensorflow_backend_matches_native_backend_forward_and_gradients() {
        use crate::NativeBackend;
        use crate::Interp;

        let shapes = [
            DenseLayerShape { in_dim: 2, out_dim: 3, relu: true },
            DenseLayerShape { in_dim: 3, out_dim: 1, relu: false },
        ];
        let w0 = vec![0.2, -0.1, 0.4, 0.3, -0.2, 0.5];
        let b0 = vec![0.3, -0.4, 0.6];
        let w1 = vec![0.3, -0.4, 0.2];
        let b1 = vec![0.15];
        let x = vec![1.0, -1.0, 0.5, 0.5];
        let y = vec![1.0, 0.0];

        let mut tf_backend = TensorFlowBackend::new();
        let tf_result = train_dense_mlp(
            &mut tf_backend, &shapes, &[w0.clone(), w1.clone()], &[b0.clone(), b1.clone()],
            2, 2, x.clone(), 2, 1, y.clone(), 1, 0.1,
        ).unwrap();

        let mut interp = Interp::new();
        let mut native_backend = NativeBackend { interp: &mut interp };
        let native_result = train_dense_mlp(
            &mut native_backend, &shapes, &[w0, w1], &[b0, b1],
            2, 2, x, 2, 1, y, 1, 0.1,
        ).unwrap();

        assert!(
            (tf_result.loss_history[0] - native_result.loss_history[0]).abs() < 1e-3,
            "first-step loss should match: tf={}, native={}",
            tf_result.loss_history[0], native_result.loss_history[0],
        );
        for (layer, (tf_w, native_w)) in tf_result.w.iter().zip(native_result.w.iter()).enumerate() {
            for (i, (a, b)) in tf_w.iter().zip(native_w.iter()).enumerate() {
                assert!((a - b).abs() < 1e-2, "layer {layer} weight {i} diverged: tf={a}, native={b}");
            }
        }
    }
}
