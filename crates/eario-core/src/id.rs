//! Typed identifiers.
//!
//! Identifiers are plain strings on the wire. The newtypes only stop a
//! dataset identifier from being passed where a state key is expected.

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates an identifier from any string-like value.
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Returns the identifier as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

string_id!(
    /// Identifies one component of a dashboard. Unique within a dashboard.
    ComponentId
);

string_id!(
    /// Identifies a dataset slot: a named place a dataset is loaded into.
    DatasetId
);

string_id!(
    /// Names one entry of the application state, written by an input
    /// component and read by the components that depend on it.
    StateKey
);
