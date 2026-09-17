//! `TensorBackend` — the swap point between Qu's own native autodiff tape
//! (the existing `Value::Tensor`/`TapeEntry` machinery elsewhere in this
//! crate) and an alternate execution engine, proven here against libtorch
//! via the optional `backend-torch` feature (`tch`, Rust bindings to
//! libtorch — see this crate's own `Cargo.toml` for why it's optional and
//! off by default).
//!
//! **Scope (2026-08-26, first vertical slice).** This is deliberately a
//! SMALL trait: just enough surface to train one real example — a stack of
//! `dense` (matmul + bias) layers, optionally followed by `relu`, full-batch
//! MSE loss, plain SGD. That's it. It does NOT cover `conv2d`/`gru_cell`/
//! attention/dropout/Adam/cross-entropy/mini-batching — see
//! `train_dense_mlp`'s own doc comment and IMPL.md's dated entry for the
//! explicit list of what a follow-on task would need to add. Matching this
//! project's own stated discipline (IMPL.md §9): prove the path on one
//! example first, expand later — re-implementing the WHOLE tensor-op
//! surface against libtorch is a separate, much larger project, not this
//! one.
//!
//! **Extended scope (2026-08-26, same day, follow-on).** The trait grew ten
//! more methods (`conv2d`, `maxpool2d`, `elementwise_mul`, `add`, `matmul`,
//! `sum_all`, `scale`, `gru_cell`, `scaled_dot_product_attention`,
//! `row_of_gru_output`) plus [`train_cnn`]/[`train_gru`]/
//! [`train_attention`] — three more `train_dense_mlp`-shaped generic loops,
//! each proving one more architecture end to end through both backends.
//! Every new op mirrors an EXISTING native builtin's own shape/semantics
//! exactly (see each trait method's own doc comment for which one) — still
//! no new math, only a real libtorch implementation of the same equations.
//! Still NOT covered: `multi_head_attention`'s multi-head split (this is
//! single-head only, matching `scaled_dot_product_attention`'s own scope),
//! attention masking, `dropout`, `layer_norm`, any optimizer but SGD on the
//! torch side, and — importantly — the `compile(..., engine="torch")`
//! LANGUAGE surface itself does not yet route to these three new
//! architectures (only the original dense/relu stack does, via
//! `sequential_fit_torch`); they are proven at this Rust
//! `TensorBackend`-generic level only, the same level `simple_cnn`/
//! `simple_rnn_classifier` already prove their OWN native-only versions at
//! (see those two builtins' own doc comments for why: `conv2d`'s `List`
//! batching and `gru_forward`'s column-shaped hidden state don't fit
//! `sequential_forward`'s row-matrix assumption on the native side either,
//! so wiring `.qu`-level conv/GRU/attention layers into `Sequential` is a
//! separate, pre-existing gap on BOTH backends, not something this task's
//! `TensorBackend` extension alone can close).
//!
//! **Two implementations:**
//! - [`crate::NativeBackend`] (in `lib.rs`, not here — it needs private
//!   access to `Interp`'s tape/`call_builtin`/`eval_grad`) is a THIN
//!   wrapper: every method just calls the existing `dense`/`relu`/`param`
//!   builtins and `eval_grad`, i.e. zero new math, zero behavior change from
//!   what `sequential_fit` already did before this trait existed.
//! - [`TorchBackend`] (gated `#[cfg(feature = "backend-torch")]`, this
//!   module) builds real `tch::Tensor`s with `requires_grad`, runs
//!   libtorch's OWN reverse-mode autograd (`Tensor::run_backward`, the same
//!   engine `loss.backward()` uses under the hood) to get gradients, and
//!   applies plain SGD via libtorch's own in-place tensor ops under a
//!   `no_grad` guard.
//!
//! [`train_dense_mlp`] is the ONE shared training loop both backends run
//! through — generic over `B: TensorBackend`, touching only the trait's
//! methods. That's the actual proof this architecture works: the same Rust
//! function, unchanged, drives either backend.

use crate::EvalError;

type R<T> = Result<T, EvalError>;

/// Minimal surface for one dense(+relu)-stack, full-batch-SGD training step.
/// See this module's own doc comment for exactly what is and isn't covered.
///
/// Every method takes `&mut self` (not a free function) because
/// `NativeBackend` needs mutable access to the interpreter's tape — a
/// stateless free-function API would work for `TorchBackend` alone but not
/// for the native side, which is the whole point of proving both behind one
/// trait.
pub trait TensorBackend {
    /// Opaque per-backend tensor handle (`Value` for `NativeBackend`,
    /// `tch::Tensor` for `TorchBackend`).
    type T: Clone;

    /// Builds a tensor from row-major `(rows, cols)` data. `data.len()` must
    /// equal `rows * cols`.
    fn from_rows(&mut self, rows: usize, cols: usize, data: Vec<f64>) -> R<Self::T>;

    /// Reads a tensor back out as row-major `(rows, cols, data)`.
    fn to_rows(&mut self, t: &Self::T) -> R<(usize, usize, Vec<f64>)>;

    /// Marks a tensor as a trainable leaf for THIS training step — the
    /// native side calls `param(...)` (a fresh tape leaf); the torch side
    /// sets `requires_grad(true)` on a fresh leaf tensor. Must be called
    /// once per parameter per step before it's used in `dense`, and the
    /// SAME returned handle (not the original) is what `dense`/
    /// `backward_and_grads` expect.
    fn param(&mut self, t: Self::T) -> R<Self::T>;

    /// `x @ w + b`, `b` broadcast over `x`'s rows.
    fn dense(&mut self, x: &Self::T, w: &Self::T, b: &Self::T) -> R<Self::T>;

    /// Elementwise `max(x, 0)`.
    fn relu(&mut self, x: &Self::T) -> R<Self::T>;

    /// `mean((pred - y) .* (pred - y))` — full-batch MSE, matching
    /// `sequential_loss`'s own `"mse"` arm exactly (see its doc comment in
    /// `lib.rs`). The only loss this first slice covers — see this module's
    /// doc comment for why cross-entropy is out of scope here.
    fn mse(&mut self, pred: &Self::T, y: &Self::T) -> R<Self::T>;

    /// One backward pass from the scalar `loss`, returning the gradient for
    /// each of `params` in the SAME order. Native: a single `eval_grad`
    /// call against the whole leaf list (exactly what `sequential_fit`
    /// already did). Torch: `Tensor::run_backward(&[loss], params, ..)` —
    /// libtorch's own reverse-mode engine, the same one a plain
    /// `loss.backward()` + reading `.grad()` per leaf would use.
    fn backward_and_grads(&mut self, loss: &Self::T, params: &[Self::T]) -> R<Vec<Self::T>>;

    /// `param - lr * grad`, returned as a new plain (non-leaf) tensor ready
    /// to be re-wrapped by `param(...)` next step.
    fn sgd_update(&mut self, param: &Self::T, grad: &Self::T, lr: f64) -> R<Self::T>;

    /// Reads a scalar (1-element) tensor back as `f64` — used for the loss
    /// history.
    fn scalar(&mut self, t: &Self::T) -> R<f64>;

    // ---- Extended surface (2026-08-26, conv2d/GRU/attention follow-on) —
    // see this module's own doc comment update below `train_dense_mlp` for
    // scope. Every one of these mirrors an EXISTING native builtin's own
    // shape/semantics exactly (single in/out-channel `conv2d`, `"valid"`-
    // only `maxpool2d`, Cho et al. GRU, single-head
    // `scaled_dot_product_attention` with no mask) — nothing here invents
    // new math, it only gives `TorchBackend` a real libtorch implementation
    // of the SAME equations `NativeBackend` already runs through
    // `call_builtin`.

    /// `a .* b`, elementwise (same shape).
    fn elementwise_mul(&mut self, a: &Self::T, b: &Self::T) -> R<Self::T>;

    /// `a + b`, elementwise (or row/scalar broadcast, matching this
    /// interpreter's own `"+"` binop rules).
    fn add(&mut self, a: &Self::T, b: &Self::T) -> R<Self::T>;

    /// `a @ b`, plain matmul (no bias) — used to build Q/K/V projections
    /// ahead of `scaled_dot_product_attention` without pulling in `dense`'s
    /// bias broadcast.
    fn matmul(&mut self, a: &Self::T, b: &Self::T) -> R<Self::T>;

    /// Sums every element of `t` down to one scalar (1x1) tensor.
    fn sum_all(&mut self, t: &Self::T) -> R<Self::T>;

    /// `t * s`, elementwise by a plain `f64` scalar (loss averaging, mainly
    /// — `1/n` over a full-dataset accumulated sum, the same "sum then
    /// divide" shape `simple_cnn_fit`'s own native loop already uses).
    fn scale(&mut self, t: &Self::T, s: f64) -> R<Self::T>;

    /// `conv2d(x, kernel, stride, padding)` for ONE single-channel image —
    /// same scope as the native `"conv2d"` builtin (`x` and `kernel` both
    /// plain 2-D `(H, W)`/`(kH, kW)` tensors, `padding` `"valid"` or
    /// `"same"` with the SAME TensorFlow-style asymmetric-pad convention
    /// `conv2d_output_shape`/`conv_dim` use — extra padding pixel goes to
    /// the bottom/right on an odd split). Batching (multiple images) is the
    /// CALLER's job, one call per image — the same shape
    /// `simple_cnn_forward` already uses (it calls the native `"conv2d"`
    /// builtin once per image in its own outer loop, never the batched-List
    /// path), not a batch dimension on this trait method.
    fn conv2d(&mut self, x: &Self::T, kernel: &Self::T, stride: usize, padding: &str) -> R<Self::T>;

    /// `maxpool2d(x, pool, stride)` for ONE single-channel feature map —
    /// `"valid"`-only (no padding), matching `maxpool2d_forward_single`'s
    /// own scope exactly.
    fn maxpool2d(&mut self, x: &Self::T, pool: usize, stride: usize) -> R<Self::T>;

    /// One GRU timestep, the same Cho et al. (2014) equations
    /// `gru_cell_compute`'s own doc comment documents:
    /// ```text
    /// z_t  = sigmoid(Wz @ x_t + Uz @ h_{t-1} + bz)
    /// r_t  = sigmoid(Wr @ x_t + Ur @ h_{t-1} + br)
    /// h~_t = tanh   (Wh @ x_t + Uh @ (r_t .* h_{t-1}) + bh)
    /// h_t  = (1 - z_t) .* h_{t-1} + z_t .* h~_t
    /// ```
    /// `x`/`h` are ROW tensors here (`(1, input_size)`/`(1, hidden_size)`,
    /// `from_rows`' own single-row convention) rather than the native
    /// column-vector convention `gru_cell_compute` uses internally — an
    /// orientation choice only, not a math difference (`x @ W^T` and `W @
    /// x` are the same dot products transposed), made because
    /// `TorchBackend::from_rows` always returns a genuine 2-D tensor (no
    /// `Value::Vec`-style row/column special case to lean on the way
    /// `NativeBackend` has). `w`'s nine fields are `(hidden_size,
    /// input_size)` (`wz`/`wr`/`wh`), `(hidden_size, hidden_size)`
    /// (`uz`/`ur`/`uh`), and `(1, hidden_size)` (`bz`/`br`/`bh`).
    fn gru_cell(&mut self, x: &Self::T, h: &Self::T, w: &GruWeights<Self::T>) -> R<Self::T>;

    /// `softmax(Q K^T / sqrt(d_k)) V` — the same equation
    /// `"scaled_dot_product_attention"`'s own doc comment documents (no
    /// mask support here — see this module's doc comment for why that's
    /// left for a follow-on). `q`/`k`/`v` are `(seq_len_q, d_k)`/
    /// `(seq_len_k, d_k)`/`(seq_len_k, d_v)`.
    fn scaled_dot_product_attention(&mut self, q: &Self::T, k: &Self::T, v: &Self::T) -> R<Self::T>;

    /// Normalizes a `gru_cell` hidden state into a `(1, hidden_size)` ROW,
    /// ready to be `dense`'s `x` argument — a real transpose on
    /// `NativeBackend` (`gru_cell_compute`'s hidden state reads back out as
    /// a column through `to_matrix`'s own default, the identical fix
    /// `simple_rnn_forward`'s own `tt(h_last)` step applies right before
    /// ITS `dense` call — see that function's doc comment) and a no-op on
    /// `TorchBackend` (already row-shaped throughout, per `gru_cell`'s own
    /// doc comment). Deliberately narrow to exactly this one spot: every
    /// OTHER op in this trait (`elementwise_mul`/`add`/`mse`/`gru_cell`
    /// itself feeding its own next timestep) auto-orients a bare row/column
    /// vector correctly already (native's `orient_vec_for_broadcast` for
    /// elementwise ops; `gru_cell_compute`'s internal chain never needs the
    /// OTHER orientation), so this is not a general-purpose transpose.
    fn row_of_gru_output(&mut self, t: &Self::T) -> R<Self::T>;

    /// A REAL, unconditional 2-D transpose (`(r, c) -> (c, r)`) — unlike
    /// `row_of_gru_output`, this always actually swaps axes on BOTH
    /// backends (no backend-specific no-op branch), since it's used
    /// wherever a caller builds a tensor in one orientation and needs the
    /// other, e.g. `train_attention`'s uniform sequence-mean-pooling row
    /// (built as an unambiguous `(seq_len, 1)` column on both backends,
    /// then transposed here to the `(1, seq_len)` row `matmul` needs as its
    /// LEFT operand).
    fn transpose(&mut self, t: &Self::T) -> R<Self::T>;

    /// Softmax cross-entropy for one sample from `n_classes` INDEPENDENT
    /// scalar logits (each a `(1,1)` tensor) — `-ln(softmax(logits)
    /// [target])`, computed in the numerically-stable log-sum-exp form
    /// `ln(sum_c exp(logit_c)) - logit_target`. Deliberately takes a
    /// per-class-scalar LIST rather than one `(1, n_classes)` row tensor:
    /// this is the exact representation `simple_cnn`'s own model-zoo
    /// architecture already uses (`simple_cnn_forward` builds one
    /// independent weight map per class, not a single dense readout — see
    /// its own doc comment), and `simple_rnn_classifier`'s follow-on
    /// (2026-08-26, `backend-torch` model-zoo wiring) reuses the SAME
    /// per-class-scalar shape for its own dense readout rather than
    /// building a genuine `(1, n_classes)` row, sidestepping
    /// `NativeBackend`'s pre-existing bare-`Value::Vec`-defaults-to-a-
    /// COLUMN row/column ambiguity (see `row_of_gru_output`'s own doc
    /// comment for the same class of bug elsewhere in this trait) entirely
    /// — no new row-orientation-safe primitive needed, this just composes
    /// `matmul`/`add`/`elementwise_mul`/`sum_all`, all already proven.
    fn softmax_cross_entropy(&mut self, logits: &[Self::T], target: usize) -> R<Self::T>;
}

/// One GRU cell's nine weight/bias tensors, generic over a backend's own
/// tensor handle `T` — see `TensorBackend::gru_cell`'s own doc comment for
/// each field's shape and the equations they feed.
pub struct GruWeights<T> {
    pub wz: T,
    pub uz: T,
    pub bz: T,
    pub wr: T,
    pub ur: T,
    pub br: T,
    pub wh: T,
    pub uh: T,
    pub bh: T,
}

/// One `dense[+relu]` layer's shape in a plain stack — `relu` is whether a
/// ReLU follows this layer's affine transform (the last layer of a
/// regression/logit-producing stack is typically `relu: false`).
pub struct DenseLayerShape {
    pub in_dim: usize,
    pub out_dim: usize,
    pub relu: bool,
}

/// Final trained weights (row-major per layer) plus the per-epoch loss
/// history — enough for a caller to rebuild whatever `Value` shape it needs
/// (`sequential_fit_torch` in `lib.rs` turns these back into the same
/// `Record{w, b}` params list `sequential_init_layer` produces).
pub struct DenseMlpResult {
    pub w: Vec<Vec<f64>>,
    pub b: Vec<Vec<f64>>,
    pub loss_history: Vec<f64>,
}

/// The one shared training loop both `NativeBackend` and `TorchBackend` run
/// through, unchanged — full-batch gradient descent for `epochs` steps over
/// a plain `dense[+relu]` stack, MSE loss, plain SGD. NOT the general
/// Sequential/`fit` machinery in `lib.rs` (that keeps its own untouched
/// native-only path with Adam/RMSprop/Adamax/lr controllers/dropout/
/// cross-entropy — see `sequential_fit`'s own doc comment for where the two
/// meet, via the `_backend` field `compile(...)` attaches).
///
/// **What this does NOT cover** (explicit, for a future extension to read
/// off): `dropout` layers, `conv2d`/`maxpool2d`/`gru_cell`/attention layers,
/// `cross_entropy` loss, `adam`/`rmsprop`/`adamax`/`lr_adaptive`/
/// `lr_plateau` optimizers, mini-batching (this is always full-batch), and
/// mixed-precision/GPU placement on the torch side (`TorchBackend` runs on
/// whatever device libtorch defaults to, `Device::Cpu` here — no `.to(Cuda)`
/// wiring).
pub fn train_dense_mlp<B: TensorBackend>(
    backend: &mut B,
    shapes: &[DenseLayerShape],
    init_w: &[Vec<f64>],
    init_b: &[Vec<f64>],
    x_rows: usize,
    x_cols: usize,
    x: Vec<f64>,
    y_rows: usize,
    y_cols: usize,
    y: Vec<f64>,
    epochs: usize,
    lr: f64,
) -> R<DenseMlpResult> {
    if shapes.len() != init_w.len() || shapes.len() != init_b.len() {
        return Err(EvalError { msg: "train_dense_mlp: shapes/init_w/init_b length mismatch".into() });
    }
    let x_t = backend.from_rows(x_rows, x_cols, x)?;
    let y_t = backend.from_rows(y_rows, y_cols, y)?;
    let mut w_t: Vec<B::T> = Vec::with_capacity(shapes.len());
    let mut b_t: Vec<B::T> = Vec::with_capacity(shapes.len());
    for (i, s) in shapes.iter().enumerate() {
        w_t.push(backend.from_rows(s.in_dim, s.out_dim, init_w[i].clone())?);
        b_t.push(backend.from_rows(1, s.out_dim, init_b[i].clone())?);
    }

    let mut loss_history = Vec::with_capacity(epochs);
    for _ in 0..epochs {
        let mut w_leaves = Vec::with_capacity(shapes.len());
        let mut b_leaves = Vec::with_capacity(shapes.len());
        for i in 0..shapes.len() {
            w_leaves.push(backend.param(w_t[i].clone())?);
            b_leaves.push(backend.param(b_t[i].clone())?);
        }
        let mut h = x_t.clone();
        for (i, s) in shapes.iter().enumerate() {
            h = backend.dense(&h, &w_leaves[i], &b_leaves[i])?;
            if s.relu {
                h = backend.relu(&h)?;
            }
        }
        let loss = backend.mse(&h, &y_t)?;
        loss_history.push(backend.scalar(&loss)?);

        let mut all_leaves = Vec::with_capacity(shapes.len() * 2);
        for i in 0..shapes.len() {
            all_leaves.push(w_leaves[i].clone());
            all_leaves.push(b_leaves[i].clone());
        }
        let grads = backend.backward_and_grads(&loss, &all_leaves)?;
        if grads.len() != all_leaves.len() {
            return Err(EvalError { msg: "train_dense_mlp: backend returned the wrong number of gradients".into() });
        }
        for i in 0..shapes.len() {
            w_t[i] = backend.sgd_update(&w_leaves[i], &grads[2 * i], lr)?;
            b_t[i] = backend.sgd_update(&b_leaves[i], &grads[2 * i + 1], lr)?;
        }
    }

    let mut w_out = Vec::with_capacity(shapes.len());
    let mut b_out = Vec::with_capacity(shapes.len());
    for i in 0..shapes.len() {
        let (_, _, wd) = backend.to_rows(&w_t[i])?;
        let (_, _, bd) = backend.to_rows(&b_t[i])?;
        w_out.push(wd);
        b_out.push(bd);
    }
    Ok(DenseMlpResult { w: w_out, b: b_out, loss_history })
}

/// Plain-data init for one GRU cell's nine weight/bias arrays (row-major
/// flat `Vec<f64>` per field, shapes as documented on
/// [`TensorBackend::gru_cell`]) — the free-function analogue of
/// [`DenseLayerShape`]/`init_w`/`init_b` in [`train_dense_mlp`], kept as its
/// own named struct (rather than 9 more positional `train_gru` parameters)
/// purely for readability at call sites.
pub struct GruInit {
    pub wz: Vec<f64>,
    pub uz: Vec<f64>,
    pub bz: Vec<f64>,
    pub wr: Vec<f64>,
    pub ur: Vec<f64>,
    pub br: Vec<f64>,
    pub wh: Vec<f64>,
    pub uh: Vec<f64>,
    pub bh: Vec<f64>,
}

/// A tiny `conv2d -> relu -> maxpool2d -> dense` image classifier/regressor,
/// generic over `B: TensorBackend` exactly like [`train_dense_mlp`] — full-
/// dataset gradient descent, one shared `kernel` (single in/out channel,
/// `"valid"` padding, stride 1) and one shared `pool`-sized non-overlapping
/// `maxpool2d`, then a single linear output unit. The "dense" head is built
/// from `elementwise_mul(pooled, class_w)` summed to a scalar plus
/// `class_b` — mathematically an ordinary dense/linear layer applied to the
/// FLATTENED pooled map (`sum(pooled .* w) == flatten(pooled) . flatten(w)`,
/// the identical dot product), reusing the exact pattern
/// `simple_cnn_forward`'s own doc comment explains (no tracked flatten/
/// stack primitive needed on either backend). MSE loss, matching
/// `train_dense_mlp`'s own loss choice so the two are directly comparable.
///
/// `images` and `labels` must be the same length and non-empty; every image
/// must share `images[0]`'s `(h, w)` (a single shared kernel/pooled-map
/// shape, like `simple_cnn`'s own one-kernel design).
pub fn train_cnn<B: TensorBackend>(
    backend: &mut B,
    images: &[(usize, usize, Vec<f64>)],
    labels: &[f64],
    kernel_hw: (usize, usize),
    init_kernel: Vec<f64>,
    pool: usize,
    init_class_w: Vec<f64>,
    init_class_b: f64,
    epochs: usize,
    lr: f64,
) -> R<Vec<f64>> {
    if images.is_empty() || images.len() != labels.len() {
        return Err(EvalError { msg: "train_cnn: images/labels length mismatch or empty".into() });
    }
    let (kh, kw) = kernel_hw;
    let n = images.len();
    let (h0, w0, _) = images[0];
    let conv_h = h0.checked_sub(kh - 1).ok_or_else(|| EvalError { msg: "train_cnn: kernel taller than image".into() })?;
    let conv_w = w0.checked_sub(kw - 1).ok_or_else(|| EvalError { msg: "train_cnn: kernel wider than image".into() })?;
    if pool == 0 || conv_h < pool || conv_w < pool {
        return Err(EvalError { msg: "train_cnn: pool_size is larger than the convolved feature map".into() });
    }
    let (ph, pw) = (conv_h / pool, conv_w / pool);
    if init_class_w.len() != ph * pw {
        return Err(EvalError { msg: format!("train_cnn: init_class_w has {} elements, expected {}x{}={}", init_class_w.len(), ph, pw, ph * pw) });
    }

    let img_t: Vec<B::T> = images.iter().map(|(h, w, d)| backend.from_rows(*h, *w, d.clone())).collect::<R<Vec<_>>>()?;
    let y_t: Vec<B::T> = labels.iter().map(|&y| backend.from_rows(1, 1, vec![y])).collect::<R<Vec<_>>>()?;
    let mut kernel_t = backend.from_rows(kh, kw, init_kernel)?;
    let mut cw_t = backend.from_rows(ph, pw, init_class_w)?;
    let mut cb_t = backend.from_rows(1, 1, vec![init_class_b])?;

    let mut loss_history = Vec::with_capacity(epochs);
    for _ in 0..epochs {
        let kernel_p = backend.param(kernel_t.clone())?;
        let cw_p = backend.param(cw_t.clone())?;
        let cb_p = backend.param(cb_t.clone())?;

        let mut total: Option<B::T> = None;
        for i in 0..n {
            let conv = backend.conv2d(&img_t[i], &kernel_p, 1, "valid")?;
            let act = backend.relu(&conv)?;
            let pooled = backend.maxpool2d(&act, pool, pool)?;
            let prod = backend.elementwise_mul(&pooled, &cw_p)?;
            let s = backend.sum_all(&prod)?;
            let out = backend.add(&s, &cb_p)?;
            let li = backend.mse(&out, &y_t[i])?;
            total = Some(match total {
                Some(t) => backend.add(&t, &li)?,
                None => li,
            });
        }
        let total = total.expect("n > 0, checked above");
        let loss = backend.scale(&total, 1.0 / n as f64)?;
        loss_history.push(backend.scalar(&loss)?);

        let leaves = [kernel_p, cw_p, cb_p];
        let grads = backend.backward_and_grads(&loss, &leaves)?;
        kernel_t = backend.sgd_update(&leaves[0], &grads[0], lr)?;
        cw_t = backend.sgd_update(&leaves[1], &grads[1], lr)?;
        cb_t = backend.sgd_update(&leaves[2], &grads[2], lr)?;
    }
    Ok(loss_history)
}

/// Final trained weights for [`train_cnn_classifier`] plus its per-epoch
/// loss history — matches `simple_cnn`'s own `ModelHandle` field shapes
/// exactly (`kernel`/`kernel_bias`/`class_w` (one entry per class)/
/// `class_b`) so a caller can rebuild a `kind="simple_cnn"` model usable by
/// the SAME `simple_cnn_predict` regardless of which backend trained it.
pub struct CnnClassifierResult {
    pub kernel: Vec<f64>,
    pub kernel_bias: f64,
    pub class_w: Vec<Vec<f64>>,
    pub class_b: Vec<f64>,
    pub loss_history: Vec<f64>,
}

/// The multi-class counterpart of [`train_cnn`] — same shared `conv2d ->
/// relu -> maxpool2d` feature map, but `n_classes` INDEPENDENT per-class
/// weight maps (`class_w`/`class_b`, one dot-product head each) and softmax
/// cross-entropy loss instead of a single MSE-regression output, matching
/// `simple_cnn`'s own native architecture exactly (see `simple_cnn_forward`'s
/// doc comment in `lib.rs`) — this is what lets `.qu`-level `simple_cnn(...,
/// engine="torch")` train through `TorchBackend` rather than only proving
/// the underlying `conv2d`/`maxpool2d` ops in isolation the way
/// [`train_cnn`] does. Returns the trained weights (not just loss history)
/// since a real model needs to be usable for `predict()` afterward.
///
/// `images`/`labels` must be the same non-empty length; `labels[i]` is a
/// 0-based class index (matching `simple_cnn_fit`'s own `y` convention, NOT
/// one-hot).
#[allow(clippy::too_many_arguments)]
pub fn train_cnn_classifier<B: TensorBackend>(
    backend: &mut B,
    images: &[(usize, usize, Vec<f64>)],
    labels: &[usize],
    kernel_hw: (usize, usize),
    init_kernel: Vec<f64>,
    init_kernel_bias: f64,
    pool: usize,
    init_class_w: &[Vec<f64>],
    init_class_b: &[f64],
    epochs: usize,
    lr: f64,
) -> R<CnnClassifierResult> {
    if images.is_empty() || images.len() != labels.len() {
        return Err(EvalError { msg: "train_cnn_classifier: images/labels length mismatch or empty".into() });
    }
    let n_classes = init_class_w.len();
    if n_classes == 0 || init_class_b.len() != n_classes {
        return Err(EvalError { msg: "train_cnn_classifier: class_w/class_b must have matching, non-zero n_classes".into() });
    }
    let (kh, kw) = kernel_hw;
    let n = images.len();
    let (h0, w0, _) = images[0];
    let conv_h = h0.checked_sub(kh - 1).ok_or_else(|| EvalError { msg: "train_cnn_classifier: kernel taller than image".into() })?;
    let conv_w = w0.checked_sub(kw - 1).ok_or_else(|| EvalError { msg: "train_cnn_classifier: kernel wider than image".into() })?;
    if pool == 0 || conv_h < pool || conv_w < pool {
        return Err(EvalError { msg: "train_cnn_classifier: pool_size is larger than the convolved feature map".into() });
    }
    let (ph, pw) = (conv_h / pool, conv_w / pool);
    for (c, w_c) in init_class_w.iter().enumerate() {
        if w_c.len() != ph * pw {
            return Err(EvalError { msg: format!("train_cnn_classifier: class_w[{c}] has {} elements, expected {}x{}={}", w_c.len(), ph, pw, ph * pw) });
        }
    }

    let img_t: Vec<B::T> = images.iter().map(|(h, w, d)| backend.from_rows(*h, *w, d.clone())).collect::<R<Vec<_>>>()?;
    let mut kernel_t = backend.from_rows(kh, kw, init_kernel)?;
    let mut kbias_t = backend.from_rows(1, 1, vec![init_kernel_bias])?;
    let mut cw_t: Vec<B::T> = init_class_w.iter().map(|w| backend.from_rows(ph, pw, w.clone())).collect::<R<Vec<_>>>()?;
    let mut cb_t: Vec<B::T> = init_class_b.iter().map(|&b| backend.from_rows(1, 1, vec![b])).collect::<R<Vec<_>>>()?;

    let mut loss_history = Vec::with_capacity(epochs);
    for _ in 0..epochs {
        let kernel_p = backend.param(kernel_t.clone())?;
        let kbias_p = backend.param(kbias_t.clone())?;
        let cw_p: Vec<B::T> = cw_t.iter().map(|w| backend.param(w.clone())).collect::<R<Vec<_>>>()?;
        let cb_p: Vec<B::T> = cb_t.iter().map(|b| backend.param(b.clone())).collect::<R<Vec<_>>>()?;

        let mut total: Option<B::T> = None;
        for i in 0..n {
            let conv = backend.conv2d(&img_t[i], &kernel_p, 1, "valid")?;
            let conv_b = backend.add(&conv, &kbias_p)?;
            let act = backend.relu(&conv_b)?;
            let pooled = backend.maxpool2d(&act, pool, pool)?;
            let mut logits = Vec::with_capacity(n_classes);
            for c in 0..n_classes {
                let prod = backend.elementwise_mul(&pooled, &cw_p[c])?;
                let s = backend.sum_all(&prod)?;
                logits.push(backend.add(&s, &cb_p[c])?);
            }
            let li = backend.softmax_cross_entropy(&logits, labels[i])?;
            total = Some(match total {
                Some(t) => backend.add(&t, &li)?,
                None => li,
            });
        }
        let total = total.expect("n > 0, checked above");
        let loss = backend.scale(&total, 1.0 / n as f64)?;
        loss_history.push(backend.scalar(&loss)?);

        let mut leaves = vec![kernel_p, kbias_p];
        leaves.extend(cw_p.iter().cloned());
        leaves.extend(cb_p.iter().cloned());
        let grads = backend.backward_and_grads(&loss, &leaves)?;
        kernel_t = backend.sgd_update(&leaves[0], &grads[0], lr)?;
        kbias_t = backend.sgd_update(&leaves[1], &grads[1], lr)?;
        for c in 0..n_classes {
            cw_t[c] = backend.sgd_update(&leaves[2 + c], &grads[2 + c], lr)?;
            cb_t[c] = backend.sgd_update(&leaves[2 + n_classes + c], &grads[2 + n_classes + c], lr)?;
        }
    }

    let (_, _, kernel_out) = backend.to_rows(&kernel_t)?;
    let (_, _, kbias_out) = backend.to_rows(&kbias_t)?;
    let mut class_w_out = Vec::with_capacity(n_classes);
    let mut class_b_out = Vec::with_capacity(n_classes);
    for c in 0..n_classes {
        let (_, _, w) = backend.to_rows(&cw_t[c])?;
        class_w_out.push(w);
        let (_, _, b) = backend.to_rows(&cb_t[c])?;
        class_b_out.push(b[0]);
    }
    Ok(CnnClassifierResult {
        kernel: kernel_out,
        kernel_bias: kbias_out[0],
        class_w: class_w_out,
        class_b: class_b_out,
        loss_history,
    })
}

/// A tiny GRU sequence regressor, generic over `B: TensorBackend`: unrolls
/// `gru_cell` across each sample's whole sequence from a fixed zero `h0`,
/// then a `dense` head on the FINAL hidden state. MSE loss, full-dataset
/// gradient descent, same overall shape as [`train_cnn`]/[`train_dense_mlp`].
///
/// `sequences[i]` is a `Vec` of per-timestep input vectors (each
/// `input_size` long); `labels[i]` is that sample's `out_dim`-long target.
pub fn train_gru<B: TensorBackend>(
    backend: &mut B,
    sequences: &[Vec<Vec<f64>>],
    labels: &[Vec<f64>],
    input_size: usize,
    hidden_size: usize,
    out_dim: usize,
    init: GruInit,
    init_w_out: Vec<f64>,
    init_b_out: Vec<f64>,
    epochs: usize,
    lr: f64,
) -> R<Vec<f64>> {
    if sequences.is_empty() || sequences.len() != labels.len() {
        return Err(EvalError { msg: "train_gru: sequences/labels length mismatch or empty".into() });
    }
    let n = sequences.len();

    let seq_t: Vec<Vec<B::T>> = sequences
        .iter()
        .map(|seq| seq.iter().map(|x| backend.from_rows(1, input_size, x.clone())).collect::<R<Vec<_>>>())
        .collect::<R<Vec<_>>>()?;
    let y_t: Vec<B::T> = labels.iter().map(|y| backend.from_rows(1, out_dim, y.clone())).collect::<R<Vec<_>>>()?;
    let h0 = backend.from_rows(1, hidden_size, vec![0.0; hidden_size])?;

    let mut wz = backend.from_rows(hidden_size, input_size, init.wz)?;
    let mut uz = backend.from_rows(hidden_size, hidden_size, init.uz)?;
    let mut bz = backend.from_rows(1, hidden_size, init.bz)?;
    let mut wr = backend.from_rows(hidden_size, input_size, init.wr)?;
    let mut ur = backend.from_rows(hidden_size, hidden_size, init.ur)?;
    let mut br = backend.from_rows(1, hidden_size, init.br)?;
    let mut wh = backend.from_rows(hidden_size, input_size, init.wh)?;
    let mut uh = backend.from_rows(hidden_size, hidden_size, init.uh)?;
    let mut bh = backend.from_rows(1, hidden_size, init.bh)?;
    let mut w_out = backend.from_rows(hidden_size, out_dim, init_w_out)?;
    let mut b_out = backend.from_rows(1, out_dim, init_b_out)?;

    let mut loss_history = Vec::with_capacity(epochs);
    for _ in 0..epochs {
        let gw = GruWeights {
            wz: backend.param(wz.clone())?,
            uz: backend.param(uz.clone())?,
            bz: backend.param(bz.clone())?,
            wr: backend.param(wr.clone())?,
            ur: backend.param(ur.clone())?,
            br: backend.param(br.clone())?,
            wh: backend.param(wh.clone())?,
            uh: backend.param(uh.clone())?,
            bh: backend.param(bh.clone())?,
        };
        let wout_p = backend.param(w_out.clone())?;
        let bout_p = backend.param(b_out.clone())?;

        let mut total: Option<B::T> = None;
        for i in 0..n {
            let mut h = h0.clone();
            for x_t in &seq_t[i] {
                h = backend.gru_cell(x_t, &h, &gw)?;
            }
            let h_row = backend.row_of_gru_output(&h)?;
            let out = backend.dense(&h_row, &wout_p, &bout_p)?;
            let li = backend.mse(&out, &y_t[i])?;
            total = Some(match total {
                Some(t) => backend.add(&t, &li)?,
                None => li,
            });
        }
        let total = total.expect("n > 0, checked above");
        let loss = backend.scale(&total, 1.0 / n as f64)?;
        loss_history.push(backend.scalar(&loss)?);

        let leaves = [gw.wz, gw.uz, gw.bz, gw.wr, gw.ur, gw.br, gw.wh, gw.uh, gw.bh, wout_p, bout_p];
        let grads = backend.backward_and_grads(&loss, &leaves)?;
        wz = backend.sgd_update(&leaves[0], &grads[0], lr)?;
        uz = backend.sgd_update(&leaves[1], &grads[1], lr)?;
        bz = backend.sgd_update(&leaves[2], &grads[2], lr)?;
        wr = backend.sgd_update(&leaves[3], &grads[3], lr)?;
        ur = backend.sgd_update(&leaves[4], &grads[4], lr)?;
        br = backend.sgd_update(&leaves[5], &grads[5], lr)?;
        wh = backend.sgd_update(&leaves[6], &grads[6], lr)?;
        uh = backend.sgd_update(&leaves[7], &grads[7], lr)?;
        bh = backend.sgd_update(&leaves[8], &grads[8], lr)?;
        w_out = backend.sgd_update(&leaves[9], &grads[9], lr)?;
        b_out = backend.sgd_update(&leaves[10], &grads[10], lr)?;
    }
    Ok(loss_history)
}

/// Final trained weights for [`train_gru_classifier`] plus its per-epoch
/// loss history — `out_w`/`out_b` come back in the SAME `(hidden_size,
/// n_classes)`/`n_classes`-long shape `simple_rnn_classifier`'s own
/// `ModelHandle` fields use (row-major, one row per hidden unit) so a
/// caller can rebuild a `kind="simple_rnn_classifier"` model usable by the
/// SAME `simple_rnn_predict` regardless of which backend trained it.
pub struct GruClassifierResult {
    pub wz: Vec<f64>, pub uz: Vec<f64>, pub bz: Vec<f64>,
    pub wr: Vec<f64>, pub ur: Vec<f64>, pub br: Vec<f64>,
    pub wh: Vec<f64>, pub uh: Vec<f64>, pub bh: Vec<f64>,
    pub out_w: Vec<f64>,
    pub out_b: Vec<f64>,
    pub loss_history: Vec<f64>,
}

/// The multi-class counterpart of [`train_gru`] — same GRU unroll over each
/// sample's sequence, but the dense readout on the final hidden state is
/// `n_classes` INDEPENDENT per-class dot products (`h_row . out_w[:,c] +
/// out_b[c]`, built from `matmul`/`add` rather than one `dense` call) fed
/// through [`TensorBackend::softmax_cross_entropy`] instead of a single MSE
/// output — matching `simple_rnn_classifier`'s own native architecture
/// (`gru_forward`'s last hidden state -> a softmax-ready dense readout,
/// see `simple_rnn_fit`'s doc comment in `lib.rs`) so `.qu`-level
/// `simple_rnn_classifier(..., engine="torch")` can train through
/// `TorchBackend`. Internally decomposing the `(hidden_size, n_classes)`
/// readout into `n_classes` separate `(hidden_size, 1)` columns (rather
/// than one `dense` matmul producing a genuine `(1, n_classes)` row) is
/// deliberate: it reuses `softmax_cross_entropy`'s existing per-class-scalar
/// contract without needing a new row-orientation-safe primitive (see that
/// trait method's own doc comment) — purely an internal computation
/// choice, the RETURNED `out_w`/`out_b` are reassembled into the ordinary
/// single-matrix shape below.
///
/// `sequences`/`labels` must be the same non-empty length; `labels[i]` is a
/// 0-based class index (matching `simple_rnn_fit`'s own `y` convention).
#[allow(clippy::too_many_arguments)]
pub fn train_gru_classifier<B: TensorBackend>(
    backend: &mut B,
    sequences: &[Vec<Vec<f64>>],
    labels: &[usize],
    input_size: usize,
    hidden_size: usize,
    n_classes: usize,
    init: GruInit,
    init_out_w: Vec<f64>,
    init_out_b: Vec<f64>,
    epochs: usize,
    lr: f64,
) -> R<GruClassifierResult> {
    if sequences.is_empty() || sequences.len() != labels.len() {
        return Err(EvalError { msg: "train_gru_classifier: sequences/labels length mismatch or empty".into() });
    }
    if n_classes == 0 || init_out_b.len() != n_classes || init_out_w.len() != hidden_size * n_classes {
        return Err(EvalError { msg: "train_gru_classifier: out_w/out_b must have matching, non-zero n_classes".into() });
    }
    let n = sequences.len();

    let seq_t: Vec<Vec<B::T>> = sequences
        .iter()
        .map(|seq| seq.iter().map(|x| backend.from_rows(1, input_size, x.clone())).collect::<R<Vec<_>>>())
        .collect::<R<Vec<_>>>()?;
    let h0 = backend.from_rows(1, hidden_size, vec![0.0; hidden_size])?;

    let mut wz = backend.from_rows(hidden_size, input_size, init.wz)?;
    let mut uz = backend.from_rows(hidden_size, hidden_size, init.uz)?;
    let mut bz = backend.from_rows(1, hidden_size, init.bz)?;
    let mut wr = backend.from_rows(hidden_size, input_size, init.wr)?;
    let mut ur = backend.from_rows(hidden_size, hidden_size, init.ur)?;
    let mut br = backend.from_rows(1, hidden_size, init.br)?;
    let mut wh = backend.from_rows(hidden_size, input_size, init.wh)?;
    let mut uh = backend.from_rows(hidden_size, hidden_size, init.uh)?;
    let mut bh = backend.from_rows(1, hidden_size, init.bh)?;
    // `init_out_w` is row-major `(hidden_size, n_classes)`; column `c`
    // (one weight per hidden unit, feeding class `c`'s scalar head) is
    // every `n_classes`-th element starting at offset `c`.
    let mut out_w_cols: Vec<B::T> = (0..n_classes)
        .map(|c| {
            let col: Vec<f64> = (0..hidden_size).map(|r| init_out_w[r * n_classes + c]).collect();
            backend.from_rows(hidden_size, 1, col)
        })
        .collect::<R<Vec<_>>>()?;
    let mut out_b: Vec<B::T> = init_out_b.iter().map(|&b| backend.from_rows(1, 1, vec![b])).collect::<R<Vec<_>>>()?;

    let mut loss_history = Vec::with_capacity(epochs);
    for _ in 0..epochs {
        let gw = GruWeights {
            wz: backend.param(wz.clone())?,
            uz: backend.param(uz.clone())?,
            bz: backend.param(bz.clone())?,
            wr: backend.param(wr.clone())?,
            ur: backend.param(ur.clone())?,
            br: backend.param(br.clone())?,
            wh: backend.param(wh.clone())?,
            uh: backend.param(uh.clone())?,
            bh: backend.param(bh.clone())?,
        };
        let out_w_p: Vec<B::T> = out_w_cols.iter().map(|w| backend.param(w.clone())).collect::<R<Vec<_>>>()?;
        let out_b_p: Vec<B::T> = out_b.iter().map(|b| backend.param(b.clone())).collect::<R<Vec<_>>>()?;

        let mut total: Option<B::T> = None;
        for i in 0..n {
            let mut h = h0.clone();
            for x_t in &seq_t[i] {
                h = backend.gru_cell(x_t, &h, &gw)?;
            }
            let h_row = backend.row_of_gru_output(&h)?;
            let mut logits = Vec::with_capacity(n_classes);
            for c in 0..n_classes {
                let s = backend.matmul(&h_row, &out_w_p[c])?;
                logits.push(backend.add(&s, &out_b_p[c])?);
            }
            let li = backend.softmax_cross_entropy(&logits, labels[i])?;
            total = Some(match total {
                Some(t) => backend.add(&t, &li)?,
                None => li,
            });
        }
        let total = total.expect("n > 0, checked above");
        let loss = backend.scale(&total, 1.0 / n as f64)?;
        loss_history.push(backend.scalar(&loss)?);

        let mut leaves = vec![gw.wz, gw.uz, gw.bz, gw.wr, gw.ur, gw.br, gw.wh, gw.uh, gw.bh];
        leaves.extend(out_w_p.iter().cloned());
        leaves.extend(out_b_p.iter().cloned());
        let grads = backend.backward_and_grads(&loss, &leaves)?;
        wz = backend.sgd_update(&leaves[0], &grads[0], lr)?;
        uz = backend.sgd_update(&leaves[1], &grads[1], lr)?;
        bz = backend.sgd_update(&leaves[2], &grads[2], lr)?;
        wr = backend.sgd_update(&leaves[3], &grads[3], lr)?;
        ur = backend.sgd_update(&leaves[4], &grads[4], lr)?;
        br = backend.sgd_update(&leaves[5], &grads[5], lr)?;
        wh = backend.sgd_update(&leaves[6], &grads[6], lr)?;
        uh = backend.sgd_update(&leaves[7], &grads[7], lr)?;
        bh = backend.sgd_update(&leaves[8], &grads[8], lr)?;
        for c in 0..n_classes {
            out_w_cols[c] = backend.sgd_update(&leaves[9 + c], &grads[9 + c], lr)?;
            out_b[c] = backend.sgd_update(&leaves[9 + n_classes + c], &grads[9 + n_classes + c], lr)?;
        }
    }

    let (_, _, wz_out) = backend.to_rows(&wz)?;
    let (_, _, uz_out) = backend.to_rows(&uz)?;
    let (_, _, bz_out) = backend.to_rows(&bz)?;
    let (_, _, wr_out) = backend.to_rows(&wr)?;
    let (_, _, ur_out) = backend.to_rows(&ur)?;
    let (_, _, br_out) = backend.to_rows(&br)?;
    let (_, _, wh_out) = backend.to_rows(&wh)?;
    let (_, _, uh_out) = backend.to_rows(&uh)?;
    let (_, _, bh_out) = backend.to_rows(&bh)?;
    let mut out_w_flat = vec![0.0; hidden_size * n_classes];
    let mut out_b_flat = Vec::with_capacity(n_classes);
    for c in 0..n_classes {
        let (_, _, col) = backend.to_rows(&out_w_cols[c])?;
        for r in 0..hidden_size {
            out_w_flat[r * n_classes + c] = col[r];
        }
        let (_, _, b) = backend.to_rows(&out_b[c])?;
        out_b_flat.push(b[0]);
    }
    Ok(GruClassifierResult {
        wz: wz_out, uz: uz_out, bz: bz_out,
        wr: wr_out, ur: ur_out, br: br_out,
        wh: wh_out, uh: uh_out, bh: bh_out,
        out_w: out_w_flat,
        out_b: out_b_flat,
        loss_history,
    })
}

/// A tiny single-head self-attention sequence regressor, generic over `B:
/// TensorBackend`: learned `Wq`/`Wk`/`Wv` project each sample's `(seq_len,
/// d_model)` input to `(seq_len, d_k)` Q/K/V, `scaled_dot_product_attention`
/// combines them, a uniform `(1, seq_len)` row of `1/seq_len` matmul'd
/// against the `(seq_len, d_k)` result pools it down to one `(1, d_k)` row
/// (mean-over-the-SEQUENCE-axis, via `matmul` — NOT `trr("row_mean", ..)`,
/// which reduces the OTHER axis: each row's own columns, `layer_norm`'s own
/// per-token-feature-vector normalization, the wrong shape here), and a
/// `dense` head (`Wo`/`bo`) produces the `out_dim` prediction. MSE loss,
/// full-dataset gradient descent — same overall shape as
/// [`train_cnn`]/[`train_gru`]. Real gradient flow into `Wq`/`Wk`/`Wv`
/// through the softmax is the actual proof this op differentiates
/// correctly end to end, not just forward.
pub fn train_attention<B: TensorBackend>(
    backend: &mut B,
    sequences: &[(usize, usize, Vec<f64>)],
    labels: &[Vec<f64>],
    d_model: usize,
    d_k: usize,
    out_dim: usize,
    init_wq: Vec<f64>,
    init_wk: Vec<f64>,
    init_wv: Vec<f64>,
    init_wo: Vec<f64>,
    init_bo: Vec<f64>,
    epochs: usize,
    lr: f64,
) -> R<Vec<f64>> {
    if sequences.is_empty() || sequences.len() != labels.len() {
        return Err(EvalError { msg: "train_attention: sequences/labels length mismatch or empty".into() });
    }
    let n = sequences.len();

    let x_t: Vec<B::T> = sequences.iter().map(|(sl, dm, d)| backend.from_rows(*sl, *dm, d.clone())).collect::<R<Vec<_>>>()?;
    // The `matmul`-based sequence-mean-pooling row (see this function's own
    // doc comment) -- a plain constant, never `param()`-wrapped, since it
    // has no weights to learn. Built as an unambiguous `(seq_len, 1)`
    // column (`from_rows` with `rows = seq_len > 1`, never `NativeBackend`'s
    // own `rows == 1` -> bare `Value::Vec` special case, which `to_matrix`
    // would silently read back out as a COLUMN when it's used as `matmul`'s
    // LEFT operand -- the exact bug this construction avoids), then
    // `transpose`d to the `(1, seq_len)` row `matmul` actually needs.
    let pool_row_t: Vec<B::T> = sequences
        .iter()
        .map(|(sl, _, _)| {
            let col = backend.from_rows(*sl, 1, vec![1.0 / *sl as f64; *sl])?;
            backend.transpose(&col)
        })
        .collect::<R<Vec<_>>>()?;
    let y_t: Vec<B::T> = labels.iter().map(|y| backend.from_rows(1, out_dim, y.clone())).collect::<R<Vec<_>>>()?;
    let mut wq = backend.from_rows(d_model, d_k, init_wq)?;
    let mut wk = backend.from_rows(d_model, d_k, init_wk)?;
    let mut wv = backend.from_rows(d_model, d_k, init_wv)?;
    let mut wo = backend.from_rows(d_k, out_dim, init_wo)?;
    let mut bo = backend.from_rows(1, out_dim, init_bo)?;

    let mut loss_history = Vec::with_capacity(epochs);
    for _ in 0..epochs {
        let wq_p = backend.param(wq.clone())?;
        let wk_p = backend.param(wk.clone())?;
        let wv_p = backend.param(wv.clone())?;
        let wo_p = backend.param(wo.clone())?;
        let bo_p = backend.param(bo.clone())?;

        let mut total: Option<B::T> = None;
        for i in 0..n {
            let q = backend.matmul(&x_t[i], &wq_p)?;
            let k = backend.matmul(&x_t[i], &wk_p)?;
            let v = backend.matmul(&x_t[i], &wv_p)?;
            let attn = backend.scaled_dot_product_attention(&q, &k, &v)?;
            let pooled = backend.matmul(&pool_row_t[i], &attn)?;
            let out = backend.dense(&pooled, &wo_p, &bo_p)?;
            let li = backend.mse(&out, &y_t[i])?;
            total = Some(match total {
                Some(t) => backend.add(&t, &li)?,
                None => li,
            });
        }
        let total = total.expect("n > 0, checked above");
        let loss = backend.scale(&total, 1.0 / n as f64)?;
        loss_history.push(backend.scalar(&loss)?);

        let leaves = [wq_p, wk_p, wv_p, wo_p, bo_p];
        let grads = backend.backward_and_grads(&loss, &leaves)?;
        wq = backend.sgd_update(&leaves[0], &grads[0], lr)?;
        wk = backend.sgd_update(&leaves[1], &grads[1], lr)?;
        wv = backend.sgd_update(&leaves[2], &grads[2], lr)?;
        wo = backend.sgd_update(&leaves[3], &grads[3], lr)?;
        bo = backend.sgd_update(&leaves[4], &grads[4], lr)?;
    }
    Ok(loss_history)
}

#[cfg(feature = "backend-torch")]
mod torch_backend {
    use super::{EvalError, TensorBackend, R};
    use tch::{Device, Kind, Tensor};

    /// `tch::Tensor` is reference-counted C++ storage under the hood but
    /// deliberately does NOT implement `Clone` itself (to force callers to
    /// choose `shallow_clone`, which shares storage, vs an actual data
    /// copy). `TensorBackend::T: Clone` needs SOME `Clone` impl (the shared
    /// training loop in this module clones tensor handles freely, e.g. the
    /// running activation `h` each layer) — a `shallow_clone` is exactly
    /// right here (same semantics as cloning an `Arc`, which `Value`'s own
    /// `Tensor`/`Mat`/`Vec` variants already do on the native side), so
    /// this newtype exists purely to give it a `Clone` impl.
    pub struct TorchT(Tensor);

    impl Clone for TorchT {
        fn clone(&self) -> Self {
            TorchT(self.0.shallow_clone())
        }
    }

    /// Real-libtorch implementation of [`TensorBackend`] via `tch`. Always
    /// `f32` (libtorch's native training precision, and what `gpu_matmul`'s
    /// own WebGPU path already uses for the same "the accelerated path
    /// trades some precision for speed" reason — see that builtin's doc
    /// comment). `device` (2026-08-26, GPU follow-on) picks where every
    /// tensor this backend CREATES lives (`Device::Cpu` or
    /// `Device::Cuda(0)`) — every other op just inherits its inputs' device
    /// the way libtorch always does, so `from_rows` (the only place a fresh
    /// tensor is built from a plain `Vec<f64>`) is the one method that
    /// actually needs to move data there explicitly; everything downstream
    /// (`param`/`dense`/`conv2d`/`gru_cell`/... ) stays on whatever device
    /// its inputs already are on with zero extra plumbing.
    pub struct TorchBackend {
        device: Device,
    }

    impl TorchBackend {
        /// CPU backend — what every pre-existing call site (`TorchBackend`
        /// as a bare unit-struct literal, before this field existed) now
        /// spells explicitly. Backward compatible in behavior, not in
        /// syntax — see this module's `Default` impl for the syntax-
        /// compatible route.
        pub fn cpu() -> Self {
            TorchBackend { device: Device::Cpu }
        }

        /// GPU backend on CUDA device `index` (almost always `0` — a single-
        /// GPU box, this repo's own dev machine included). Does NOT check
        /// `tch::Cuda::is_available()` itself (a cheap, cacheable check a
        /// caller typically wants to do ONCE up front with its own clear
        /// error message, e.g. `sequential_fit_torch`'s `device="cuda"`
        /// handling in `lib.rs` — this constructor stays infallible and
        /// trusts the caller).
        pub fn cuda(index: usize) -> Self {
            TorchBackend { device: Device::Cuda(index) }
        }

        pub fn new(device: Device) -> Self {
            TorchBackend { device }
        }
    }

    impl Default for TorchBackend {
        fn default() -> Self {
            Self::cpu()
        }
    }

    /// `"same"` padding's leading (`before`)/trailing (`after`) pad amount
    /// for ONE spatial dimension -- the exact same split `conv_dim`
    /// computes on the native side (`needed.saturating_sub(n) / 2` leading,
    /// the odd leftover pixel trailing), just returning BOTH halves instead
    /// of only `pad_before` since `tch` needs an explicit `constant_pad_nd`
    /// rather than getting the trailing half "for free" from
    /// `conv2d_single`'s own out-of-range-is-zero indexing.
    fn same_pad(n: i64, k: i64, stride: i64) -> (i64, i64) {
        let out = ((n + stride - 1) / stride).max(1);
        let needed = (out - 1) * stride + k;
        let total = (needed - n).max(0);
        let before = total / 2;
        (before, total - before)
    }

    impl TensorBackend for TorchBackend {
        type T = TorchT;

        fn from_rows(&mut self, rows: usize, cols: usize, data: Vec<f64>) -> R<TorchT> {
            if data.len() != rows * cols {
                return Err(EvalError { msg: format!("TorchBackend: expected {} elements for a {rows}x{cols} tensor, got {}", rows * cols, data.len()) });
            }
            let data_f32: Vec<f32> = data.iter().map(|&v| v as f32).collect();
            Ok(TorchT(Tensor::from_slice(&data_f32).reshape([rows as i64, cols as i64]).to_device(self.device)))
        }

        fn to_rows(&mut self, t: &TorchT) -> R<(usize, usize, Vec<f64>)> {
            // `f_copy_data` reads through host memory, so a GPU-resident
            // tensor must come back to `Cpu` first — a no-op `.to_device`
            // call (device-to-same-device) when `t` is already on CPU.
            let t = t.0.to_device(Device::Cpu).to_kind(Kind::Float).contiguous();
            let sizes = t.size();
            let (rows, cols) = match sizes.as_slice() {
                [r, c] => (*r as usize, *c as usize),
                [n] => (1usize, *n as usize),
                other => return Err(EvalError { msg: format!("TorchBackend: expected a 1D or 2D tensor, got shape {other:?}") }),
            };
            let numel = rows * cols;
            let mut buf = vec![0f32; numel];
            t.reshape([numel as i64]).f_copy_data(&mut buf, numel)
                .map_err(|err| EvalError { msg: format!("TorchBackend: reading tensor data failed: {err}") })?;
            Ok((rows, cols, buf.into_iter().map(|v| v as f64).collect()))
        }

        fn param(&mut self, t: TorchT) -> R<TorchT> {
            // A fresh leaf: detach from whatever graph `t` came from (the
            // previous step's SGD update, itself computed under `no_grad`
            // below, so `t` normally has no graph anyway) and turn on
            // gradient tracking for THIS step.
            Ok(TorchT(t.0.detach().set_requires_grad(true)))
        }

        fn dense(&mut self, x: &TorchT, w: &TorchT, b: &TorchT) -> R<TorchT> {
            Ok(TorchT(x.0.matmul(&w.0) + &b.0))
        }

        fn relu(&mut self, x: &TorchT) -> R<TorchT> {
            Ok(TorchT(x.0.relu()))
        }

        fn mse(&mut self, pred: &TorchT, y: &TorchT) -> R<TorchT> {
            let diff = &pred.0 - &y.0;
            Ok(TorchT((&diff * &diff).mean(Kind::Float)))
        }

        fn backward_and_grads(&mut self, loss: &TorchT, params: &[TorchT]) -> R<Vec<TorchT>> {
            // Libtorch's own reverse-mode autograd engine — the same one
            // `loss.backward()` invokes; `run_backward` is the functional
            // form (`torch.autograd.grad` in Python) so gradients come back
            // as plain tensors instead of being stashed on `.grad()`
            // fields, matching this trait's "return the grads" contract.
            let param_tensors: Vec<&Tensor> = params.iter().map(|p| &p.0).collect();
            let grads = Tensor::f_run_backward(&[&loss.0], &param_tensors, false, false)
                .map_err(|err| EvalError { msg: format!("TorchBackend: backward pass failed: {err}") })?;
            Ok(grads.into_iter().map(TorchT).collect())
        }

        fn sgd_update(&mut self, param: &TorchT, grad: &TorchT, lr: f64) -> R<TorchT> {
            // Plain SGD (`param -= lr * grad`) under `no_grad` -- this IS
            // libtorch's own update math (elementwise ops running through
            // the same ATen kernels `tch::nn::Sgd` itself bottoms out at),
            // just without a `VarStore` to own the parameters -- this
            // trait's params are plain owned tensors passed back to the
            // caller each step, not a persistent `nn::Module`, so
            // `nn::Optimizer` (which mutates a `VarStore` in place) doesn't
            // fit this functional per-call shape.
            let _guard = tch::no_grad_guard();
            Ok(TorchT(&param.0 - &grad.0 * lr))
        }

        fn scalar(&mut self, t: &TorchT) -> R<f64> {
            Ok(t.0.double_value(&[]))
        }

        fn elementwise_mul(&mut self, a: &TorchT, b: &TorchT) -> R<TorchT> {
            Ok(TorchT(&a.0 * &b.0))
        }

        fn add(&mut self, a: &TorchT, b: &TorchT) -> R<TorchT> {
            Ok(TorchT(&a.0 + &b.0))
        }

        fn matmul(&mut self, a: &TorchT, b: &TorchT) -> R<TorchT> {
            Ok(TorchT(a.0.matmul(&b.0)))
        }

        fn sum_all(&mut self, t: &TorchT) -> R<TorchT> {
            Ok(TorchT(t.0.sum(Kind::Float)))
        }

        fn scale(&mut self, t: &TorchT, s: f64) -> R<TorchT> {
            Ok(TorchT(&t.0 * s))
        }

        fn conv2d(&mut self, x: &TorchT, kernel: &TorchT, stride: usize, padding: &str) -> R<TorchT> {
            let (in_h, in_w) = match x.0.size().as_slice() {
                [h, w] => (*h, *w),
                other => return Err(EvalError { msg: format!("TorchBackend conv2d: expected a 2-D (H, W) image, got shape {other:?}") }),
            };
            let (k_h, k_w) = match kernel.0.size().as_slice() {
                [h, w] => (*h, *w),
                other => return Err(EvalError { msg: format!("TorchBackend conv2d: expected a 2-D (kH, kW) kernel, got shape {other:?}") }),
            };
            let stride_i = stride as i64;
            if stride_i < 1 {
                return Err(EvalError { msg: "TorchBackend conv2d: stride must be at least 1".into() });
            }
            let (pad_t, pad_b, pad_l, pad_r) = match padding {
                "valid" => (0, 0, 0, 0),
                "same" => {
                    let (pt, pb) = same_pad(in_h, k_h, stride_i);
                    let (pl, pr) = same_pad(in_w, k_w, stride_i);
                    (pt, pb, pl, pr)
                }
                other => return Err(EvalError { msg: format!("TorchBackend conv2d: padding must be \"valid\" or \"same\", found \"{other}\"") }),
            };
            if in_h + pad_t + pad_b < k_h || in_w + pad_l + pad_r < k_w {
                return Err(EvalError { msg: "TorchBackend conv2d: kernel is larger than the (padded) input".into() });
            }
            let x4 = x.0.reshape([1, 1, in_h, in_w]);
            let x4 = if pad_t + pad_b + pad_l + pad_r > 0 {
                x4.constant_pad_nd(&[pad_l, pad_r, pad_t, pad_b])
            } else {
                x4
            };
            let k4 = kernel.0.reshape([1, 1, k_h, k_w]);
            let out = x4.conv2d::<Tensor>(&k4, None, [stride_i, stride_i], [0, 0], [1, 1], 1);
            let sizes = out.size();
            let (out_h, out_w) = (sizes[2], sizes[3]);
            Ok(TorchT(out.reshape([out_h, out_w])))
        }

        fn maxpool2d(&mut self, x: &TorchT, pool: usize, stride: usize) -> R<TorchT> {
            let (in_h, in_w) = match x.0.size().as_slice() {
                [h, w] => (*h, *w),
                other => return Err(EvalError { msg: format!("TorchBackend maxpool2d: expected a 2-D (H, W) feature map, got shape {other:?}") }),
            };
            let (pool_i, stride_i) = (pool as i64, stride as i64);
            if pool_i < 1 || stride_i < 1 {
                return Err(EvalError { msg: "TorchBackend maxpool2d: pool_size and stride must be at least 1".into() });
            }
            if in_h < pool_i || in_w < pool_i {
                return Err(EvalError { msg: "TorchBackend maxpool2d: pool_size is larger than the input".into() });
            }
            let x4 = x.0.reshape([1, 1, in_h, in_w]);
            let out = x4.max_pool2d([pool_i, pool_i], [stride_i, stride_i], [0, 0], [1, 1], false);
            let sizes = out.size();
            Ok(TorchT(out.reshape([sizes[2], sizes[3]])))
        }

        fn gru_cell(&mut self, x: &TorchT, h: &TorchT, w: &super::GruWeights<TorchT>) -> R<TorchT> {
            // Cho et al. (2014), the SAME equations `gru_cell_compute`'s own
            // doc comment documents -- see `TensorBackend::gru_cell`'s doc
            // comment for why `x`/`h` are rows here rather than columns
            // (orientation only, not a math difference).
            let z = (x.0.matmul(&w.wz.0.transpose(0, 1)) + h.0.matmul(&w.uz.0.transpose(0, 1)) + &w.bz.0).sigmoid();
            let r = (x.0.matmul(&w.wr.0.transpose(0, 1)) + h.0.matmul(&w.ur.0.transpose(0, 1)) + &w.br.0).sigmoid();
            let rh = &r * &h.0;
            let h_tilde = (x.0.matmul(&w.wh.0.transpose(0, 1)) + rh.matmul(&w.uh.0.transpose(0, 1)) + &w.bh.0).tanh();
            let one_minus_z = &z * (-1.0) + 1.0;
            Ok(TorchT(&one_minus_z * &h.0 + &z * &h_tilde))
        }

        fn scaled_dot_product_attention(&mut self, q: &TorchT, k: &TorchT, v: &TorchT) -> R<TorchT> {
            let d_k = match k.0.size().as_slice() {
                [_, d] => *d as f64,
                other => return Err(EvalError { msg: format!("TorchBackend scaled_dot_product_attention: expected a 2-D (seq_len, d_k) k, got shape {other:?}") }),
            };
            let scores = q.0.matmul(&k.0.transpose(0, 1)) / d_k.sqrt();
            let probs = scores.softmax(-1, Kind::Float);
            Ok(TorchT(probs.matmul(&v.0)))
        }

        fn row_of_gru_output(&mut self, t: &TorchT) -> R<TorchT> {
            // No-op: this backend's `gru_cell` already keeps `h` as a `(1,
            // hidden_size)` row throughout (see that method's own doc
            // comment) — only `NativeBackend` needs an actual transpose
            // here.
            Ok(t.clone())
        }

        fn transpose(&mut self, t: &TorchT) -> R<TorchT> {
            Ok(TorchT(t.0.transpose(0, 1).contiguous()))
        }

        fn softmax_cross_entropy(&mut self, logits: &[TorchT], target: usize) -> R<TorchT> {
            if target >= logits.len() {
                return Err(EvalError { msg: format!("TorchBackend: label {target} is out of range for {} classes", logits.len()) });
            }
            // Concatenate the `n_classes` independent `(1,1)` scalar
            // tensors into one `(1, n_classes)` row -- `Tensor::cat` is a
            // real differentiable op in libtorch's autograd graph, so the
            // gradient flows back into each ORIGINAL scalar tensor exactly
            // as if the row had been built from a single matmul, matching
            // `softmax_ce_from_list`'s own multi-input LSE reasoning on the
            // native side (see this trait method's own doc comment for why
            // the input is a list of scalars rather than one row to begin
            // with).
            let parts: Vec<&Tensor> = logits.iter().map(|l| &l.0).collect();
            let row = Tensor::cat(&parts, 1);
            let target_t = Tensor::from_slice(&[target as i64]).to_device(self.device);
            Ok(TorchT(row.cross_entropy_for_logits(&target_t)))
        }
    }
}

#[cfg(feature = "backend-torch")]
pub use torch_backend::TorchBackend;

#[cfg(all(test, feature = "backend-torch"))]
mod torch_tests {
    use super::*;

    /// XOR: not linearly separable, so this only converges if `dense` +
    /// `relu` + backprop through BOTH layers actually works — a smoke test
    /// that `TorchBackend`'s graph (leaf `param()` -> `dense` -> `relu` ->
    /// `dense` -> `mse`) is wired correctly end to end through real
    /// libtorch autograd.
    #[test]
    fn torch_backend_trains_xor_down_to_a_low_loss() {
        let shapes = [
            DenseLayerShape { in_dim: 2, out_dim: 8, relu: true },
            DenseLayerShape { in_dim: 8, out_dim: 1, relu: false },
        ];
        // Fixed, deterministic init (not random) so this test can't
        // flake on an unlucky seed landing in a dead-ReLU region.
        let w0 = vec![
            0.5, -0.3, 0.2, 0.4, -0.5, 0.1, -0.2, 0.3,
            -0.4, 0.6, -0.1, 0.5, 0.3, -0.6, 0.2, -0.4,
        ];
        let b0 = vec![0.0; 8];
        let w1 = vec![0.3, -0.4, 0.5, -0.2, 0.4, -0.3, 0.2, -0.5];
        let b1 = vec![0.0];
        let x = vec![0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0];
        let y = vec![0.0, 1.0, 1.0, 0.0];

        let mut backend = TorchBackend::cpu();
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
        let mut backend = TorchBackend::cpu();
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        let t = backend.from_rows(2, 3, data.clone()).unwrap();
        let (r, c, out) = backend.to_rows(&t).unwrap();
        assert_eq!((r, c), (2, 3));
        assert_eq!(out, data);
    }
}
