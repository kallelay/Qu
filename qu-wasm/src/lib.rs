//! Thin browser ABI over the authoritative `qu-core` reference semantics.
//!
//! Parsing and full program execution will join this boundary next.  Keeping
//! this layer thin prevents the browser from becoming a second implementation.

use wasm_bindgen::prelude::*;

use qu_core::matrix::Matrix;
use serde::Serialize;

#[wasm_bindgen]
pub fn engine_capabilities() -> String {
    qu_core::capabilities().to_owned()
}

#[wasm_bindgen]
pub fn inclusive_range(start: f64, stop: f64, step: f64) -> Result<Vec<f64>, JsValue> {
    qu_core::inclusive_range(start, stop, step, qu_core::DEFAULT_ELEMENT_LIMIT)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen]
pub fn mean(values: &[f64]) -> Result<f64, JsValue> {
    qu_core::mean(values).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen]
pub fn rms(values: &[f64]) -> Result<f64, JsValue> {
    qu_core::rms(values).map_err(|error| JsValue::from_str(&error.to_string()))
}

/// Returns `[re0, im0, re1, im1, ...]` to avoid allocating JS objects per bin.
#[wasm_bindgen]
pub fn fft_interleaved(values: &[f64]) -> Result<Vec<f64>, JsValue> {
    qu_core::fft_real(values)
        .map(|spectrum| {
            spectrum
                .bins()
                .iter()
                .flat_map(|bin| [bin.re, bin.im])
                .collect()
        })
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

#[derive(Serialize)]
struct PseudoInverseReport {
    /// Pseudoinverse shape. An input `(rows, cols)` produces `(cols, rows)`.
    rows: usize,
    cols: usize,
    /// Column-major matrix data, matching Qu, BLAS/LAPACK, and the core ABI.
    values: Vec<f64>,
    rank: usize,
    tolerance: f64,
    condition_number: f64,
    singular_values: Vec<f64>,
}

fn matrix_from_browser(values: &[f64], rows: u32, cols: u32) -> Result<Matrix, JsValue> {
    let rows = rows as usize;
    let cols = cols as usize;
    let expected = rows
        .checked_mul(cols)
        .ok_or_else(|| JsValue::from_str("matrix dimensions overflow addressable memory"))?;
    if values.len() != expected {
        return Err(JsValue::from_str(&format!(
            "matrix shape {rows}x{cols} needs {expected} values, received {}",
            values.len()
        )));
    }
    Matrix::from_col_major_checked(rows, cols, values.to_vec())
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// Moore-Penrose pseudoinverse for browser callers. The returned JSON carries
/// column-major values plus rank/tolerance/conditioning diagnostics so Studio
/// can explain a numerically singular result instead of merely displaying it.
#[wasm_bindgen]
pub fn pseudo_inverse_report(
    values: &[f64],
    rows: u32,
    cols: u32,
    relative_tolerance: Option<f64>,
) -> Result<String, JsValue> {
    let input = matrix_from_browser(values, rows, cols)?;
    let result = qu_core::linalg::pseudo_inverse(&input, relative_tolerance)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let (output_rows, output_cols) = result.matrix.shape();
    serde_json::to_string(&PseudoInverseReport {
        rows: output_rows,
        cols: output_cols,
        values: result.matrix.as_slice().to_vec(),
        rank: result.rank,
        tolerance: result.tolerance,
        condition_number: result.condition_number,
        singular_values: result.singular_values,
    })
    .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// Minimum-norm least-squares solve `x = pinv(A) * b`. Both inputs and the
/// result use column-major storage; `b` may contain several right-hand sides.
#[wasm_bindgen]
pub fn least_squares_col_major(
    coefficients: &[f64],
    coefficient_rows: u32,
    coefficient_cols: u32,
    observations: &[f64],
    observation_cols: u32,
    relative_tolerance: Option<f64>,
) -> Result<Vec<f64>, JsValue> {
    let a = matrix_from_browser(coefficients, coefficient_rows, coefficient_cols)?;
    let b = matrix_from_browser(observations, coefficient_rows, observation_cols)?;
    qu_core::linalg::least_squares(&a, &b, relative_tolerance)
        .map(|solution| solution.as_slice().to_vec())
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

/// Vertex array for the premade box mesh: `[x0, y0, z0, x1, y1, z1, ...]`.
#[wasm_bindgen]
pub fn box_vertices(width: f64, height: f64, depth: f64) -> Result<Vec<f64>, JsValue> {
    let mesh =
        qu_core::geometry::Mesh3::box_mesh(qu_core::geometry::Vec3::new(width, height, depth))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
    Ok(flat_vertices(&mesh.vertices))
}

/// Triangle index array for the premade box mesh.
#[wasm_bindgen]
pub fn box_triangles(width: f64, height: f64, depth: f64) -> Result<Vec<u32>, JsValue> {
    let mesh =
        qu_core::geometry::Mesh3::box_mesh(qu_core::geometry::Vec3::new(width, height, depth))
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
    Ok(mesh.triangles.into_iter().flatten().collect())
}

/// Compiles any flat vertex array to the broad-phase box hull used by physics.
#[wasm_bindgen]
pub fn compile_box_hull(vertices: &[f64]) -> Result<Vec<f64>, JsValue> {
    if vertices.is_empty() || !vertices.len().is_multiple_of(3) {
        return Err(JsValue::from_str(
            "vertex array length must be a nonzero multiple of 3",
        ));
    }
    let positions = vertices
        .chunks_exact(3)
        .map(|value| qu_core::geometry::Vec3::new(value[0], value[1], value[2]))
        .collect();
    let mesh = qu_core::geometry::Mesh3::new(positions, Vec::new())
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let hull = qu_core::geometry::compile_hull(&mesh, qu_core::geometry::HullMode::Box)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    Ok(flat_vertices(&hull.vertices))
}

fn flat_vertices(vertices: &[qu_core::geometry::Vec3]) -> Vec<f64> {
    vertices
        .iter()
        .flat_map(|vertex| [vertex.x, vertex.y, vertex.z])
        .collect()
}

#[wasm_bindgen]
pub fn where_indices(mask: &[u8]) -> Vec<u32> {
    let logical: Vec<_> = mask.iter().map(|value| *value != 0).collect();
    qu_core::selection::where_indices(&logical)
        .into_iter()
        .map(|index| index as u32)
        .collect()
}

#[wasm_bindgen]
pub fn gather(values: &[f64], indices: &[u32]) -> Result<Vec<f64>, JsValue> {
    let indices: Vec<_> = indices.iter().map(|index| *index as usize).collect();
    qu_core::selection::gather(values, &indices)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen]
pub fn masked_fill(values: &[f64], mask: &[u8], replacement: f64) -> Result<Vec<f64>, JsValue> {
    let mut output = values.to_vec();
    let mask: Vec<_> = mask.iter().map(|value| *value != 0).collect();
    qu_core::selection::masked_fill(&mut output, &mask, replacement)
        .map(|_| output)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_boundary_keeps_fft_layout_compact() {
        let output = fft_interleaved(&[1.0, 0.0, 0.0, 0.0]).unwrap();
        assert_eq!(output, vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn wasm_boundary_exposes_mesh_and_compiled_hull_arrays() {
        assert_eq!(box_vertices(2.0, 2.0, 2.0).unwrap().len(), 8 * 3);
        assert_eq!(box_triangles(2.0, 2.0, 2.0).unwrap().len(), 12 * 3);
        let hull = compile_box_hull(&[-2.0, -1.0, 0.0, 4.0, 3.0, 2.0]).unwrap();
        assert_eq!(hull.len(), 8 * 3);
    }

    #[test]
    fn wasm_boundary_exposes_logical_selection_semantics() {
        let values = masked_fill(&[-2.0, 1.0, 4.0], &[1, 0, 0], 0.0).unwrap();
        let indices = where_indices(&[0, 0, 1]);
        assert_eq!(indices, vec![2]);
        assert_eq!(gather(&values, &indices).unwrap(), vec![4.0]);
    }

    #[test]
    fn wasm_boundary_exposes_pseudoinverse_diagnostics() {
        // A = [1 2; 2 4; 3 6], column-major: exactly rank one.
        let report = pseudo_inverse_report(&[1.0, 2.0, 3.0, 2.0, 4.0, 6.0], 3, 2, None).unwrap();
        let report: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(report["rows"], 2);
        assert_eq!(report["cols"], 3);
        assert_eq!(report["rank"], 1);
        assert_eq!(report["values"].as_array().unwrap().len(), 6);
        assert_eq!(report["singular_values"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn wasm_boundary_solves_least_squares_with_column_major_inputs() {
        // y = 1 + 2x at x = 0,1,2. A = [1 x], b = y.
        let solution = least_squares_col_major(
            &[1.0, 1.0, 1.0, 0.0, 1.0, 2.0],
            3,
            2,
            &[1.0, 3.0, 5.0],
            1,
            None,
        )
        .unwrap();
        assert!((solution[0] - 1.0).abs() < 1.0e-12);
        assert!((solution[1] - 2.0).abs() < 1.0e-12);
    }
}
