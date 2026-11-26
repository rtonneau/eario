# Named computations and derived datasets

_ADR 0012, recorded 2026-11-26 (session 2026-11-26__custom-rust-logic)._

<!-- Format adapted from mattpocock-skills' domain-modeling (ADR-FORMAT.md): https://github.com/mattpocock/skills.git -->

Status: Accepted

Dashboards could only combine built-in components (ADR 0001). A developer now registers named Rust computations, `fn(&ComputeContext) -> Result<DataFrame, ComputeError>`, and a dataset slot can take its table from one (`DatasetSlot.source = Computed { function, inputs, params }`), so the specification stays serializable and every component reads a derived dataset like an imported one. Derived datasets are computed lazily and cached by the revisions of their inputs, except that one with inputs bound to it is recomputed during the change so their state can be reconciled; the context object, rather than an explicit argument list, leaves room for progress, cancellation and Python implementations (ADR 0007) without breaking authors' functions.
