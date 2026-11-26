# eario glossary

Eario is a local-first Rust framework for interactive dashboards and data exploration: a dashboard is described in Rust, compiled into one executable, and served to a generic browser frontend from the loopback interface.

<!-- Format adapted from mattpocock-skills' domain-modeling (GLOSSARY-FORMAT.md): https://github.com/mattpocock/skills.git -->

<!--
How to add a term (the moment it is resolved, not in a batch at the end):

**Term**:
One or two sentences: what it IS, not what it does.
_Avoid_: the other words people use for it

- Be opinionated: one word per concept; list the others under _Avoid_.
- Only terms specific to this project. General programming concepts and
  implementation details don't belong here: this is a glossary, not a spec.
- Group terms under "### <area>" subheadings once natural clusters emerge.
-->

## Language

### Custom logic

**Computation**:
A named Rust function registered with the dashboard that turns input tables and parameters into one table. The specification refers to it by name only.
_Avoid_: callback, plugin, transform, custom statistic (a custom statistic is a computation that returns a small table)

**Derived dataset**:
A dataset slot whose table is the output of a computation rather than an import. Components read it exactly like an imported dataset.
_Avoid_: computed dataset, virtual dataset, view

**Compute context**:
What a computation receives: its named input tables and its named parameters, typed on access.
_Avoid_: inputs, args

**Parameter**:
A named value a computation reads, bound either to a state entry or to a literal in the specification.
_Avoid_: argument, option
