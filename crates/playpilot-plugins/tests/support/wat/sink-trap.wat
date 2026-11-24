;; event-sink: traps on every delivery.
(module
  (memory (export "memory") 1)
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (i32.const 1024))
  (func (export "playpilot:fleet/event-sink@1.0.0#deliver")
    (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
    unreachable))
