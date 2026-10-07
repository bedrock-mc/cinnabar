(component
  (import "cinnabar:extension/camera@0.1.0" (instance $camera
    (export "set-preserve-teleport-rotation" (func (param "enabled" bool)
      (result (result (error string)))))))
  (alias export $camera "set-preserve-teleport-rotation" (func $preserve))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $preserve) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "preserve" (func $preserve (param i32 i32)))
    (import "host" "memory" (memory 1))
    (global $count (mut i32) (i32.const 0))
    (func (export "init"))
    (func (export "frame")
      global.get $count i32.const 1 i32.add global.set $count
      $FRAME))
  (core instance $host
    (export "preserve" (func $lower))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
