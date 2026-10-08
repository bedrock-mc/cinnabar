(component
  (import "$HUD" (instance $hud
    (export "set-label" (func (param "text" string) (result (result (error string)))))))
  (import "$INPUT" (instance $input
    (export "demo-pressed" (func (result bool)))))
  (import "$ENVIRONMENT" (instance $environment
    (export "set-time-override" (func (param "ticks" (option u32)) (result (result (error string)))))))
  (alias export $environment "set-time-override" (func $set-time))
  (alias export $hud "set-label" (func $set-label))
  (alias export $input "demo-pressed" (func $demo-pressed))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.tee $old
      local.get 3 i32.add global.set $next local.get $old)
    (data (i32.const 0) "$TEXT"))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-label (canon lower (func $set-label) (memory $memory) (realloc $realloc)))
  (core func $lower-time (canon lower (func $set-time) (memory $memory) (realloc $realloc)))
  (core func $lower-input (canon lower (func $demo-pressed)))
  (core module $code
    (import "host" "time" (func $time (param i32 i32 i32)))
    (import "host" "label" (func $label (param i32 i32 i32)))
    (import "host" "pressed" (func $pressed (result i32)))
    (import "host" "memory" (memory 1))
    (func (export "init") i32.const 0 i32.const $LENGTH i32.const 512 call $label)
    (func (export "frame") $FRAME))
  (core instance $host
    (export "time" (func $lower-time))
    (export "label" (func $lower-label))
    (export "pressed" (func $lower-input))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
