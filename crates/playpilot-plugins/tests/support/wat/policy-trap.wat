;; policy-check: traps on every call.
(module
  (memory (export "memory") 1)
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (i32.const 1024))
  (func (export "playpilot:fleet/policy-check@1.0.0#check-command")
    (param i32 i32 i32 i32) (result i32)
    unreachable))
