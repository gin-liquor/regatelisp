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

Stage 4.5 lowers register expressions as fixed-width typed hardware values. An
unannotated integer may use the surrounding register type, so `(next (+ count 1))`
keeps an 8-bit `count` and emits `8'd1`; explicit `meta ((width N))` annotations
remain checked rather than resized implicitly. Width extension and truncation are
not performed.

## SystemVerilog CLI (Stage 4.5b completion)

Compile a hardware module from standard input with `--emit-systemverilog`:

```powershell
Get-Content -Raw examples/counter_sv.lisp |
    cargo run --quiet -- --emit-systemverilog
```

The command writes only generated SystemVerilog to standard output, so it can be
redirected directly to a file:

```powershell
Get-Content -Raw examples/counter_sv.lisp |
    cargo run --quiet -- --emit-systemverilog |
    Set-Content -Encoding utf8 examples/counter_sv.sv
```

## Comparisons and hardware `if` (Stage 4.5c)

Hardware expressions support `=`, `!=`, `<`, `<=`, `>`, and `>=`. Both operands
must have the same width; an unannotated integer inherits the other operand's
width. A comparison produces an unsigned 1-bit value.

Hardware `if` is a value expression that emits a SystemVerilog conditional
operator. Its condition must be 1-bit, and both branches must have the same type.
The destination type is propagated into unannotated integer branches:

```lisp
(next
  (if (= count 255)
      0
      (+ count 1)))
```

For a complete counter example:

```powershell
Get-Content -Raw examples/wrapping_counter_sv.lisp |
    cargo run --quiet -- --emit-systemverilog
```

## FSM enums and cases (Stage 6)

Module-local `enum` declarations define fixed-width unsigned symbolic constants.
They are emitted as `localparam logic`; they are not nominal enum types.

Value-producing `case` requires a final `else` and emits deterministic nested
conditional expressions. Inside `clocked`, `case-do` emits a SystemVerilog `case`
statement. Omitting its `else` retains register values when no key matches.
Different arms may update the same register, while duplicate updates on one
execution path are rejected.

```powershell
Get-Content -Raw examples/fsm_sv.lisp |
    cargo run --quiet -- --emit-systemverilog
```

## Vector operations (Stage 7)

Hardware expressions support fixed, compile-time bit and vector operations:

```lisp
(bit value index)          ; index 0 is the LSB
(slice value high low)     ; inclusive [high:low]
(concat upper lower)       ; upper occupies the most-significant bits
(resize value new-width)   ; zero-extends unsigned values, sign-extends signed values
```

`bit`, `slice`, and `concat` always produce unsigned values. `resize` preserves
the operand's signedness and is the explicit way to change width; ordinary
assignments still reject implicit width or signedness conversions.

```powershell
Get-Content -Raw examples/vector_ops_sv.lisp |
    cargo run --quiet -- --emit-systemverilog
```

## Code data and generated symbols (Stage 8)

`quote` returns source structure as a `Datum` without evaluating it. `quasiquote`
does the same except that an `unquote` at the matching nesting depth is evaluated
and converted back into code data:

```lisp
(quote (+ 1 2))

(let ((value 42))
  (quasiquote
    (assign result (unquote value))))
```

`gensym` returns a generated symbol with identity distinct from every ordinary
symbol and from every other generated symbol. Its optional prefix must evaluate
to an ordinary symbol Datum:

```lisp
(gensym)
(gensym (quote temporary))
```

Generated-symbol numbering belongs to one evaluation session, so a fresh
interpreter starts deterministically at `g__g0`. Datum conversion is structural,
does not reparse text, and intentionally drops syntax properties.

Only the full S-expression forms are supported; reader abbreviations such as
quote/backquote/comma and `unquote-splicing` are not implemented. Macros are
reserved for Stage 9, and syntax-object/property APIs for Stage 9.5.

```powershell
Get-Content -Raw examples/quasiquote.lisp | cargo run --quiet
```
