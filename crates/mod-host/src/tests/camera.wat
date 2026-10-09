(component
  (import "$CAMERA" (instance $camera
    (export "set-preserve-teleport-rotation" (func (param "enabled" bool)
      (result (result (error string)))))
    (export "set-view-scale" (func (param "fov-scale" f32) (param "look-scale" f32)
      (result (result (error string)))))))
  (alias export $camera "set-preserve-teleport-rotation" (func $preserve))
  (alias export $camera "set-view-scale" (func $view-scale))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 4096))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $preserve) (memory $memory) (realloc $realloc)))
  (core func $scale-lower (canon lower (func $view-scale) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "preserve" (func $preserve (param i32 i32)))
    (import "host" "view-scale" (func $view-scale (param f32 f32 i32)))
    (import "host" "memory" (memory 1))
    (global $count (mut i32) (i32.const 0))
    (func (export "init"))
    (func (export "frame")
      global.get $count i32.const 1 i32.add global.set $count
      $FRAME))
  (core instance $host
    (export "preserve" (func $lower))
    (export "view-scale" (func $scale-lower))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
