;; policy-check: refuses any command containing "rm -rf", allows the rest.
(module
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 1024))
  (data (i32.const 16) "rm -rf")
  (data (i32.const 32) "example policy: rm -rf is not allowed")

  (func (export "cabi_realloc") (param i32 i32) (param $align i32) (param $size i32) (result i32)
    (local $p i32)
    (local.set $p
      (i32.and
        (i32.add (global.get $heap) (i32.sub (local.get $align) (i32.const 1)))
        (i32.xor (i32.sub (local.get $align) (i32.const 1)) (i32.const -1))))
    (global.set $heap (i32.add (local.get $p) (local.get $size)))
    (local.get $p))

  (func $contains (param $hp i32) (param $hl i32) (param $np i32) (param $nl i32) (result i32)
    (local $i i32) (local $j i32)
    (block $absent
      (loop $outer
        (br_if $absent (i32.gt_u (i32.add (local.get $i) (local.get $nl)) (local.get $hl)))
        (local.set $j (i32.const 0))
        (block $mismatch
          (loop $inner
            (if (i32.eq (local.get $j) (local.get $nl)) (then (return (i32.const 1))))
            (br_if $mismatch
              (i32.ne
                (i32.load8_u (i32.add (i32.add (local.get $hp) (local.get $i)) (local.get $j)))
                (i32.load8_u (i32.add (local.get $np) (local.get $j)))))
            (local.set $j (i32.add (local.get $j) (i32.const 1)))
            (br $inner)))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $outer)))
    (i32.const 0))

  ;; verdict at offset 0: u8 case, then (ptr, len) of the refuse string at 4 and 8.
  (func (export "playpilot:fleet/policy-check@1.0.0#check-command")
    (param i32 i32) (param $cp i32) (param $cl i32) (result i32)
    (if (call $contains (local.get $cp) (local.get $cl) (i32.const 16) (i32.const 6))
      (then
        (i32.store8 (i32.const 0) (i32.const 1))
        (i32.store (i32.const 4) (i32.const 32))
        (i32.store (i32.const 8) (i32.const 37)))
      (else
        (i32.store8 (i32.const 0) (i32.const 0))))
    (i32.const 0)))
