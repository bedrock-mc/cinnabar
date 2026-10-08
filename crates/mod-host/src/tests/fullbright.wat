(component
  (import "$ENVIRONMENT" (instance $environment
    (export "set-fullbright" (func (param "enabled" bool) (result (result (error string)))))
    (export "set-time-override" (func (param "ticks" (option u32)) (result (result (error string)))))))
  (alias export $environment "set-fullbright" (func $fullbright))
  (alias export $environment "set-time-override" (func $time))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 32768))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-fullbright (canon lower (func $fullbright) (memory $memory) (realloc $realloc)))
  (core func $lower-time (canon lower (func $time) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "fullbright" (func $fullbright (param i32 i32)))
    (import "host" "time" (func $time (param i32 i32 i32)))
    (import "host" "memory" (memory 1))
    (func $on
      i32.const 1 i32.const 512 call $fullbright
      i32.const 512 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func $off
      i32.const 0 i32.const 512 call $fullbright
      i32.const 512 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func $clock i32.const 1 i32.const 6000 i32.const 512 call $time)
    (func (export "init") $INIT)
    (func (export "frame") $FRAME))
  (core instance $host
    (export "fullbright" (func $lower-fullbright))
    (export "time" (func $lower-time))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
