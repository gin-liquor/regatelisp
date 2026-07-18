# ReGateLisp

ReGateLisp is a small Lisp implemented in Rust with an explicit compiler pipeline:
reader AST, expansion, Core AST, lowering, verification, and IR evaluation.

Every reader and Core S-expression can carry optional syntax properties. Properties
are currently an internal data model only: ordinary Lisp source remains unchanged,
the parser produces empty property sets, and properties do not affect evaluation or IR.

Stage 1 provides the `Properties` and `PropertyValue` API plus property preservation
from source-derived reader nodes to their corresponding Core nodes. No property
annotation syntax is implemented yet.
