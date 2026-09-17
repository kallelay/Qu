//! Logical masks, `where` indices, gathering, and indexed assignment.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Clone, Debug, PartialEq)]
pub enum SelectionError {
    LengthMismatch { values: usize, mask: usize },
    IndexOutOfBounds { index: usize, length: usize },
    ReplacementLength { selected: usize, replacement: usize },
}

impl Display for SelectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::LengthMismatch { values, mask } => {
                write!(
                    formatter,
                    "mask length {mask} does not match value length {values}"
                )
            }
            Self::IndexOutOfBounds { index, length } => {
                write!(
                    formatter,
                    "index {index} is outside 0..{}",
                    length.saturating_sub(1)
                )
            }
            Self::ReplacementLength {
                selected,
                replacement,
            } => write!(
                formatter,
                "replacement length {replacement} does not match selection length {selected}"
            ),
        }
    }
}

impl Error for SelectionError {}

/// Zero-based indices of every `true` element, in stable source order.
pub fn where_indices(mask: &[bool]) -> Vec<usize> {
    mask.iter()
        .enumerate()
        .filter_map(|(index, selected)| selected.then_some(index))
        .collect()
}

pub fn gather<T: Copy>(values: &[T], indices: &[usize]) -> Result<Vec<T>, SelectionError> {
    indices
        .iter()
        .map(|index| {
            values
                .get(*index)
                .copied()
                .ok_or(SelectionError::IndexOutOfBounds {
                    index: *index,
                    length: values.len(),
                })
        })
        .collect()
}

pub fn masked_fill<T: Copy>(
    values: &mut [T],
    mask: &[bool],
    replacement: T,
) -> Result<(), SelectionError> {
    if values.len() != mask.len() {
        return Err(SelectionError::LengthMismatch {
            values: values.len(),
            mask: mask.len(),
        });
    }
    for (value, selected) in values.iter_mut().zip(mask) {
        if *selected {
            *value = replacement;
        }
    }
    Ok(())
}

pub fn indexed_assign<T: Copy>(
    values: &mut [T],
    indices: &[usize],
    replacement: &[T],
) -> Result<(), SelectionError> {
    if indices.len() != replacement.len() {
        return Err(SelectionError::ReplacementLength {
            selected: indices.len(),
            replacement: replacement.len(),
        });
    }
    let length = values.len();
    for (index, value) in indices.iter().zip(replacement) {
        let target = values
            .get_mut(*index)
            .ok_or(SelectionError::IndexOutOfBounds {
                index: *index,
                length,
            })?;
        *target = *value;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_and_gather_keep_source_order() {
        let values = [0.0, 1.0, 4.0, 2.0, 5.0];
        let mask: Vec<_> = values.iter().map(|value| *value > 3.0).collect();
        let indices = where_indices(&mask);
        assert_eq!(indices, vec![2, 4]);
        assert_eq!(gather(&values, &indices).unwrap(), vec![4.0, 5.0]);
    }

    #[test]
    fn masked_fill_supports_relu_style_assignment() {
        let mut values = [-2.0, -1.0, 3.0];
        masked_fill(&mut values, &[true, true, false], 0.0).unwrap();
        assert_eq!(values, [0.0, 0.0, 3.0]);
    }

    #[test]
    fn indexed_assignment_rejects_shape_mismatch() {
        let mut values = [1, 2, 3];
        assert_eq!(
            indexed_assign(&mut values, &[0, 2], &[9]),
            Err(SelectionError::ReplacementLength {
                selected: 2,
                replacement: 1
            })
        );
    }
}
