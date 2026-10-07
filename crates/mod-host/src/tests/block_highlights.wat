(component
  (import "$RENDER" (instance $render
    (type $c (record (field "r" f32) (field "g" f32) (field "b" f32) (field "a" f32)))
    (export "rgba" (type $rgba (eq $c)))
    (type $s (record (field "identifiers" (list string)) (field "range" f32) (field "color" $rgba)))
    (export "block-highlight-spec" (type $spec (eq $s)))
    (export "set-block-highlights" (func (param "spec" (option $spec)) (result (result (error string)))))))
  (alias export $render "set-block-highlights" (func $set))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 32768))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $set) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "set" (func $set (param i32 i32 i32 f32 f32 f32 f32 f32 i32)))
    (import "host" "memory" (memory 1))
    (data (i32.const 1024) "minecraft:ancient_debris")
    (func $selection
      i32.const 2048 i32.const 1024 i32.store
      i32.const 2052 i32.const 24 i32.store
      i32.const 1 i32.const 2048 i32.const 1 f32.const 64
      f32.const 1 f32.const 0.1 f32.const 0.7 f32.const 0.8 i32.const 512 call $set
      i32.const 512 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func $clear
      i32.const 0 i32.const 0 i32.const 0 f32.const 0
      f32.const 0 f32.const 0 f32.const 0 f32.const 0 i32.const 512 call $set)
    (func (export "init") $INIT)
    (func (export "frame") $FRAME))
  (core instance $host (export "set" (func $lower)) (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
