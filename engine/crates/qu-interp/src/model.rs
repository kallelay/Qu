//! Fitted-model values (`Value::Model`) — a first, deliberately scoped-down
//! slice of the spec's §37.2 "model protocol" (`fit`/`predict`/`score`
//! obeyed uniformly by every estimator). The `predict`/`score`/`pipeline`/
//! `fit` builtins that dispatch on a `ModelHandle`'s `kind` live in
//! `qu-interp`'s own `lib.rs` (they need `numeric::linalg` and the rest of
//! the builtin machinery already in scope there); this module only defines
//! the value shape itself.
//!
//! Deliberately NOT built: the spec's `import ml.linear`/`ml.cluster`
//! namespacing (Qu has no import/module system yet) or the `pipeline
//! stage name = ... end pipeline` block syntax (a real grammar change,
//! bigger and riskier than the feature itself). Estimators are flat,
//! `_model`-suffixed builtins instead (`ols_model`, `ridge_model`, ...) so
//! they don't collide with the pre-existing one-shot `ridge`/`kmeans`/`pca`
//! functions, and a pipeline names its transform stages by the same
//! function-name-as-string convention `pmap`/`spawn` already use.

use crate::Value;

/// A fitted model, OR an unfitted pipeline spec (see `stages`) — the single
/// value shape every estimator in the model protocol returns.
#[derive(Clone, Debug)]
pub struct ModelHandle {
    /// Discriminates what `predict`/`score`/`fit` do with this handle:
    /// `"ols"`, `"ridge"`, `"kmeans"`, `"pca"`, `"pipeline_spec"` (returned
    /// directly by `pipeline(...)`, not yet fitted), or `"pipeline_fitted"`
    /// (returned by `pipeline_spec.fit(X, y)`).
    pub kind: String,
    /// Named, read-only fields a script reads via `m.field` (`m.coef`,
    /// `m.centers`, ...) — an ordered `Vec`, not a `HashMap`, so field
    /// listings in error messages are deterministic. A `"pipeline_fitted"`
    /// model's only field is `"model"`, the wrapped final estimator.
    pub fields: Vec<(String, Value)>,
    /// Pipeline-only: transform stage function names, in application
    /// order, to replay on new data before delegating to the final
    /// estimator (the last entry passed to `pipeline(...)`, wrapped in
    /// `fields["model"]` once fitted). Empty for every non-pipeline model.
    pub stages: Vec<String>,
}

impl ModelHandle {
    pub fn new(kind: impl Into<String>, fields: Vec<(String, Value)>) -> Self {
        ModelHandle { kind: kind.into(), fields, stages: Vec::new() }
    }

    pub fn with_stages(kind: impl Into<String>, fields: Vec<(String, Value)>, stages: Vec<String>) -> Self {
        ModelHandle { kind: kind.into(), fields, stages }
    }

    pub fn field(&self, name: &str) -> Option<&Value> {
        self.fields.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    pub fn field_names(&self) -> String {
        self.fields.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")
    }
}
