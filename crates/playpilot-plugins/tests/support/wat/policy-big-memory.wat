;; policy-check: asks for 64 MiB of memory, past the host's cap.
(module
  (memory (export "memory") 1024)
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (i32.const 1024))
  (func (export "playpilot:fleet/policy-check@1.0.0#check-command")
    (param i32 i32 i32 i32) (result i32)
    (i32.store8 (i32.const 0) (i32.const 0))
    (i32.const 0)))
