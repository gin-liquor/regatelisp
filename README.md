# ReGateLisp

ReGateLisp is a small Lisp implemented in Rust with an explicit compiler pipeline:
reader AST, expansion, Core AST, lowering, verification, and IR evaluation.

Every reader and Core S-expression can carry optional syntax properties. Properties
are currently an internal data model only: ordinary Lisp source remains unchanged,
the parser produces empty property sets, and properties do not affect evaluation or IR.

Properties are optional. Stage 2 adds the `meta` S-expression, which attaches
free-form, inert properties to one expression:

```lisp
(+ 1 2)

(meta ((width 8)
       (target vhdl))
  (+ a b))

(meta ((author "user")
       (purpose audio-dsp)
       (options (fast experimental)))
  expression)
```

`meta` is an ordinary S-expression. Its property values are stored as syntax data,
not evaluated; unknown names are retained; and properties do not change evaluation
in Stage 2. Future analyzers and a VHDL backend may choose to interpret them.

## Combinational hardware (Stage 3)

```lisp
(module adder
  (ports
    (input  (meta ((width 8)) a))
    (input  (meta ((width 8)) b))
    (output (meta ((width 8)) y)))
  (assign y (+ a b)))
```

`compile_systemverilog` lowers this through a backend-independent Hardware IR and
emits SystemVerilog. Stage 3 supports combinational assignments only. `width` is
required on ports and integer constants; `signed` is optional and defaults to false.
There are no implicit width or signedness conversions. A future VHDL backend can
consume the same Hardware IR.

## Sequential registers (Stage 4)

Registers are declared inside `registers` and use the same metadata type syntax as ports:

```lisp
(register state (meta ((width 1)) state)
  (clock clk rising)
  (reset sync rst high 0)
  (enable en)
  (next d))
```

Use `falling` instead of `rising` for a negedge register. Resets are synchronous,
support active-high and active-low polarity, and take priority over enables. Register
values are connected to output ports through ordinary `assign` forms. Asynchronous
reset is not supported in Stage 4.
