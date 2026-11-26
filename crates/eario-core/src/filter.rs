//! Row filter applied to a dataset.

use serde::{Deserialize, Serialize};

/// Keeps the rows of a dataset whose `column` satisfies `op value`.
///
/// Rows where the column is missing never match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Filter {
    /// Name of the column the filter reads.
    pub column: String,
    /// Comparison applied to each value of the column.
    pub op: FilterOp,
    /// Right-hand side of the comparison.
    pub value: FilterValue,
}

/// Comparison operator of a [`Filter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    /// Equal to the value.
    Eq,
    /// Different from the value.
    Ne,
    /// Strictly greater than the value (numeric columns).
    Gt,
    /// Greater than or equal to the value (numeric columns).
    Ge,
    /// Strictly less than the value (numeric columns).
    Lt,
    /// Less than or equal to the value (numeric columns).
    Le,
    /// Contains the value as a substring (text columns).
    Contains,
}

/// Right-hand side of a [`Filter`]: a number or a piece of text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(untagged)]
pub enum FilterValue {
    /// Numeric value, compared with numeric columns.
    Number(f64),
    /// Text value, compared with text columns.
    Text(String),
}
