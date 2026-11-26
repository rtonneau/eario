# 0007. Python through managed subprocesses and Arrow

Status: Proposed. Nothing is implemented.

## Context

Laboratories have analysis code in Python. Eario should eventually run such
scripts and use NumPy, SciPy or scikit-learn, while a pure Rust application
keeps working on a machine without Python.

## Proposed direction

Run Python as a **managed subprocess**, exchanging tables as **Arrow IPC**:

- Eario starts a Python interpreter chosen by the user (a path or an
  environment), sends the input tables as Arrow IPC streams and receives
  results the same way. Polars and pyarrow both read and write this format
  without conversion.
- The integration lives in an optional extension crate
  (`extensions/python`), behind a Cargo feature. The core never links Python.
- Scripts never run implicitly. Opening a session or a project does not
  execute anything; the user starts each run, and sees which script and
  interpreter will be used.

It plugs into the same extension point as custom Rust computations
(ADR 0001): a named function taking tables and parameters and returning
tables. A Python script is one implementation of that function.

## Why a subprocess rather than an embedded interpreter

| Concern | Subprocess | Embedded (PyO3) |
| --- | --- | --- |
| Packaging | The executable stays self-contained | Must link a specific libpython; breaks single-file distribution |
| Environment | Uses the user's existing environment as is | Version and ABI must match the build |
| Isolation | A crash or a leak stays in the child | Takes the application down |
| Cancellation | Kill the process | Cooperative only |
| Data transfer | One serialization through Arrow IPC | Zero-copy possible |
| Security | Can be confined by the operating system | Shares the address space |

The cost is start-up time and one copy of the data per call, acceptable for
analyses that take longer than an interaction. PyO3 stays possible later for
cases where that copy matters; the function-level abstraction does not
depend on the choice.

## Open questions

- Discovering and validating a Python environment (venv, conda, uv), and
  recording it for reproducibility.
- The protocol between Eario and the child: a small runner script shipped
  with the extension, and how parameters, progress and errors travel.
- Sandboxing, given that a script is arbitrary code.
- Whether Arrow IPC needs additional Polars features that are costly to build.
