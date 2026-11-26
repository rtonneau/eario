# 0003. Polars data frames for tables, behind `eario-data`

Status: Accepted

## Context

The first targets are tables from CSV and Excel files, from thousands to a
few million rows. Later data will include spectra, images and
multidimensional arrays, which are not tables. Writing a dataframe engine is
excluded.

## Decision

- A tabular dataset is a Polars `DataFrame`. A `Dataset` pairs it with its
  provenance (source name, format, worksheets).
- The imported table is immutable. Queries derive new tables; cloning a
  `DataFrame` shares its columns.
- Polars is confined to `eario-data`. `eario_data::query` returns the plain
  types of `eario-core`, so the runtime, the frontend contract and MCP do not
  depend on Polars types (the runtime passes `DataFrame` handles through
  without inspecting them).
- New formats implement the `TableReader` trait and are added to a
  `ReaderRegistry`. CSV, TSV and XLSX (through calamine) are built in.
- Only the eager Polars API and its CSV reader are enabled. The lazy engine
  is not: nothing uses it yet, and it roughly doubles the build.
- Row counts are `u32` in the contract, matching the default Polars index
  size (about 4.29 billion rows). Larger tables are rejected on import.

### Excel type inference

calamine gives typed cells. A column takes the narrowest type holding all its
non-empty cells: integer when every number is whole (Excel stores all numbers
as floats), float, boolean, otherwise text. Empty and error cells are missing
values. The first used row is the header; blank or repeated names are made
unique.

## Alternatives

- **Arrow record batches directly** (arrow-rs). The interchange format of
  choice, but filtering, statistics and type inference would have to be
  written.
- **DataFusion.** A full SQL engine; more than is needed now and a larger
  dependency. A candidate if SQL queries are wanted.
- **Polars' own Excel support** exists only in the Python package; the Rust
  crate has none, hence calamine.

## Consequences

- Polars dominates build time and changes its API between minor versions.
  Confinement to one crate keeps upgrades local.
- A dataset must fit in memory, and so must the uploaded file during import.
- Grouping, joins, pivots and calculated columns will need the lazy engine or
  more eager features; each feature combination must be checked to compile
  (see the technical assessment).

## Open questions

- **Non-tabular data.** The intended shape is an enum of data objects (table,
  n-dimensional array, ...) held by `App`, each with its own queries. It is
  not introduced now: with one variant it would be an abstraction without a
  use.
- **Dates and times.** Temporal Polars types are disabled. Excel dates are
  read as serial numbers and CSV dates as text. This is a known limitation.
- **Import settings.** `ImportOptions` carries only the worksheet. Header
  row, separator, decimal comma, encoding and type overrides are to come.
- **Larger than memory.** Polars streaming and Parquet are the expected
  route; nothing is designed.
