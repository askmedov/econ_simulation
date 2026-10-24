//! Tables of numbers by place: one row per place (village or town), one
//! column per product or business, as numpy's 2-D arrays in Python.

use serde::{Deserialize, Serialize};

/// A row-major table of `rows` x `cols` numbers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

impl Grid {
    pub fn new(rows: usize, cols: usize, value: f64) -> Grid {
        Grid { rows, cols, data: vec![value; rows * cols] }
    }

    pub fn zeros(rows: usize, cols: usize) -> Grid {
        Grid::new(rows, cols, 0.0)
    }

    /// From nested rows (each of the same length).
    pub fn from_rows(rows: &[Vec<f64>], cols: usize) -> Grid {
        let mut data = Vec::with_capacity(rows.len() * cols);
        for row in rows {
            assert_eq!(row.len(), cols, "rows of different lengths");
            data.extend_from_slice(row);
        }
        Grid { rows: rows.len(), cols, data }
    }

    /// Each row the same: `row` repeated `rows` times.
    pub fn tile(row: &[f64], rows: usize) -> Grid {
        let mut data = Vec::with_capacity(rows * row.len());
        for _ in 0..rows {
            data.extend_from_slice(row);
        }
        Grid { rows, cols: row.len(), data }
    }

    /// Each column the same: `column[i]` all along row i.
    pub fn spread(column: &[f64], cols: usize) -> Grid {
        let mut data = Vec::with_capacity(column.len() * cols);
        for &v in column {
            data.extend(std::iter::repeat_n(v, cols));
        }
        Grid { rows: column.len(), cols, data }
    }

    #[inline]
    pub fn get(&self, row: usize, col: usize) -> f64 {
        self.data[row * self.cols + col]
    }

    #[inline]
    pub fn set(&mut self, row: usize, col: usize, value: f64) {
        self.data[row * self.cols + col] = value;
    }

    #[inline]
    pub fn at(&mut self, row: usize, col: usize) -> &mut f64 {
        &mut self.data[row * self.cols + col]
    }

    pub fn row(&self, row: usize) -> &[f64] {
        &self.data[row * self.cols..(row + 1) * self.cols]
    }

    pub fn row_mut(&mut self, row: usize) -> &mut [f64] {
        &mut self.data[row * self.cols..(row + 1) * self.cols]
    }

    pub fn column(&self, col: usize) -> Vec<f64> {
        (0..self.rows).map(|r| self.get(r, col)).collect()
    }

    pub fn set_column(&mut self, col: usize, values: &[f64]) {
        for (r, &v) in values.iter().enumerate() {
            self.set(r, col, v);
        }
    }

    pub fn row_sums(&self) -> Vec<f64> {
        (0..self.rows).map(|r| self.row(r).iter().sum()).collect()
    }

    pub fn column_sums(&self) -> Vec<f64> {
        let mut sums = vec![0.0; self.cols];
        for r in 0..self.rows {
            for (s, v) in sums.iter_mut().zip(self.row(r)) {
                *s += v;
            }
        }
        sums
    }

    pub fn sum(&self) -> f64 {
        self.data.iter().sum()
    }

    /// A new table of `f` of each pair of cells of `self` and `other`.
    pub fn zip_map(&self, other: &Grid, f: impl Fn(f64, f64) -> f64) -> Grid {
        assert_eq!((self.rows, self.cols), (other.rows, other.cols));
        Grid {
            rows: self.rows,
            cols: self.cols,
            data: self.data.iter().zip(&other.data).map(|(&a, &b)| f(a, b)).collect(),
        }
    }

    pub fn map(&self, f: impl Fn(f64) -> f64) -> Grid {
        Grid { rows: self.rows, cols: self.cols, data: self.data.iter().map(|&a| f(a)).collect() }
    }

    /// Columns `cols` of each row, in that order (numpy's `grid[:, cols]`).
    pub fn take_columns(&self, cols: &[usize]) -> Grid {
        let mut out = Grid::zeros(self.rows, cols.len());
        for r in 0..self.rows {
            for (j, &c) in cols.iter().enumerate() {
                out.set(r, j, self.get(r, c));
            }
        }
        out
    }
}

/// A table of `rows` x `cols` x `depth` numbers (places x businesses x products).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grid3 {
    pub rows: usize,
    pub cols: usize,
    pub depth: usize,
    pub data: Vec<f64>,
}

impl Grid3 {
    pub fn zeros(rows: usize, cols: usize, depth: usize) -> Grid3 {
        Grid3 { rows, cols, depth, data: vec![0.0; rows * cols * depth] }
    }

    #[inline]
    pub fn get(&self, r: usize, c: usize, d: usize) -> f64 {
        self.data[(r * self.cols + c) * self.depth + d]
    }

    #[inline]
    pub fn at(&mut self, r: usize, c: usize, d: usize) -> &mut f64 {
        &mut self.data[(r * self.cols + c) * self.depth + d]
    }
}

/// Sum `value(i)` into the group `group[i]` of `n` groups (numpy's
/// `bincount(group, weights=...)`).
#[inline]
pub fn sum_by(n: usize, group: &[u32], value: impl Fn(usize) -> f64) -> Vec<f64> {
    let mut sums = vec![0.0; n];
    for (i, &g) in group.iter().enumerate() {
        sums[g as usize] += value(i);
    }
    sums
}

/// Sum `values` into the groups `group` (bincount with weights).
pub fn sum_values_by(n: usize, group: &[u32], values: &[f64]) -> Vec<f64> {
    sum_by(n, group, |i| values[i])
}

/// `a / b`, or `fallback` where `b` is not positive (numpy's
/// `divide(a, b, out=fallback, where=b > 0)`).
#[inline]
pub fn div_or(a: f64, b: f64, fallback: f64) -> f64 {
    if b > 0.0 {
        a / b
    } else {
        fallback
    }
}

#[inline]
pub fn clip(x: f64, low: f64, high: f64) -> f64 {
    x.max(low).min(high)
}

/// numpy's `average(values, weights=weights)`.
pub fn average(values: &[f64], weights: &[f64]) -> f64 {
    let total: f64 = weights.iter().sum();
    values.iter().zip(weights).map(|(v, w)| v * w).sum::<f64>() / total
}

pub fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

/// The largest of `values` (0 for none).
pub fn max_of(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
}
