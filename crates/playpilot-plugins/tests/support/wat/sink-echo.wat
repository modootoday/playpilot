;; event-sink: accepts events of type "com.example.ok"; for any other type it
;; returns err(<the type it received>), which shows the event arrived intact.
(module
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 1024))
  (data (i32.const 16) "com.example.ok")

  (func (export "cabi_realloc") (param i32 i32) (param $align i32) (param $size i32) (result i32)
    (local $p i32)
    (local.set $p
      (i32.and
        (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
        (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))
    (global.set $heap (i32.add (local.get $p) (local.get $size)))
    (local.get $p))

  (func $equal (param $a i32) (param $al i32) (param $b i32) (param $bl i32) (result i32)
    (local $i i32)
    (if (i32.ne (local.get $al) (local.get $bl)) (then (return (i32.const 0))))
    (block $done
      (loop $next
        (br_if $done (i32.eq (local.get $i) (local.get $al)))
        (if (i32.ne
              (i32.load8_u (i32.add (local.get $a) (local.get $i)))
              (i32.load8_u (i32.add (local.get $b) (local.get $i))))
          (then (return (i32.const 0))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))
    (i32.const 1))

  ;; event = (id, type, source, time, data-json), each string as (ptr, len).
  ;; result<_, string> at offset 0: u8 case, then (ptr, len) at 4 and 8.
  (func (export "playpilot:fleet/event-sink@1.0.0#deliver")
    (param i32 i32) (param $tp i32) (param $tl i32) (param i32 i32 i32 i32 i32 i32) (result i32)
    (if (call $equal (local.get $tp) (local.get $tl) (i32.const 16) (i32.const 14))
      (then
        (i32.store8 (i32.const 0) (i32.const 0)))
      (else
        (i32.store8 (i32.const 0) (i32.const 1))
        (i32.store (i32.const 4) (local.get $tp))
        (i32.store (i32.const 8) (local.get $tl))))
    (i32.const 0)))
