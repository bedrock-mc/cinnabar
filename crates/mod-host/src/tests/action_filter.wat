(component
  (import "cinnabar:extension/input@0.1.0" (instance $input
    (export "demo-pressed" (func (result bool)))))
  (core func $pressed (canon lower (func $input "demo-pressed")))
  (core module $m
    (import "env" "pressed" (func $pressed (result i32)))
    (memory (export "memory") 1)
    (global $heap (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32) (param $size i32) (result i32)
      (local $ptr i32) global.get $heap local.tee $ptr local.get $size i32.add global.set $heap local.get $ptr)
    (data (i32.const 64) "\01\01\01\01\01\01\01\01")
    (func (export "init"))
    (func (export "frame"))
    (func (export "filter") (param i64 i64 i32 i32 i32 i32 i32) (param $count i32) (result i32)
      $BODY
      i32.const 0 i32.const 64 i32.store
      i32.const 4 $COUNT i32.store
      i32.const 8 i32.const 0 i32.store
      i32.const 12 i32.const 0 i32.store
      i32.const 0))
  (core instance $env (export "pressed" (func $pressed)))
  (core instance $i (instantiate $m (with "env" (instance $env))))
  (alias core export $i "memory" (core memory $memory))
  (alias core export $i "realloc" (core func $realloc))
  (func (export "init") (canon lift (core func $i "init")))
  (func (export "frame") (canon lift (core func $i "frame")))
  (import "cinnabar:extension/action-filter@0.1.0" (instance $types
    (type $a (enum "attack" "item-use"))
    (export "filtered-action" (type $action (eq $a)))
    (type $k (enum "press" "release" "repeat"))
    (export "transition-kind" (type $kind (eq $k)))
    (type $t (record (field "action" $action) (field "kind" $kind) (field "held-after" bool)))
    (export "transition" (type $transition (eq $t)))
    (type $h (record (field "attack" bool) (field "item-use" bool)))
    (export "held" (type $held (eq $h)))
    (type $f (record (field "epoch" u64) (field "time-us" u64)
      (field "physical" $held) (field "logical" $held) (field "transitions" (list $transition))))
    (export "action-frame" (type (eq $f)))
    (type $v (record (field "accepted" (list bool)) (field "release" $held)))
    (export "action-verdict" (type (eq $v)))))
  (alias export $types "action-frame" (type $frame))
  (alias export $types "action-verdict" (type $verdict))
  (func (export "filter-actions") (param "frame" $frame) (result $verdict)
    (canon lift (core func $i "filter") (memory $memory) (realloc $realloc))))
