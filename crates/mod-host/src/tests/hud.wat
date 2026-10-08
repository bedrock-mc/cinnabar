(component
  (import "$HUD" (instance $hud
    (export "set-content" (func (param "json" string) (result (result (error string)))))
    (export "set-crosshair" (func (param "json" string) (result (result (error string)))))))
  (alias export $hud "set-content" (func $content))
  (alias export $hud "set-crosshair" (func $crosshair))
  (core module $memory-module
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) i32.const 32768))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-content (canon lower (func $content) (memory $memory) (realloc $realloc)))
  (core func $lower-crosshair (canon lower (func $crosshair) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "content" (func $content (param i32 i32 i32)))
    (import "host" "crosshair" (func $crosshair (param i32 i32 i32)))
    (import "host" "memory" (memory 1))
    (data (i32.const 0) "$CONTENT")
    (data (i32.const 4096) "$CROSSHAIR")
    (func $cards
      i32.const 0 i32.const $CONTENT_LENGTH i32.const 16384 call $content
      i32.const 16384 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func $cursor
      i32.const 4096 i32.const $CROSSHAIR_LENGTH i32.const 16384 call $crosshair
      i32.const 16384 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    (func $clear
      i32.const 0 i32.const 0 i32.const 16384 call $content
      i32.const 0 i32.const 0 i32.const 16384 call $crosshair)
    (func (export "init") $INIT)
    (func (export "frame") $FRAME))
  (core instance $host
    (export "content" (func $lower-content))
    (export "crosshair" (func $lower-crosshair))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
