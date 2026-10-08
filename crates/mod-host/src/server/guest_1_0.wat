(component
  (import "cinnabar:server-experience/ui@1.0.0" (instance $ui
    (export "set-widget" (func (param "id" string) (param "text" string) (result (result (error string)))))))
  (alias export $ui "set-widget" (func $set-widget))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.tee $old
      local.get 3 i32.add global.set $next local.get $old)
    (data (i32.const 0) "status")
    (data (i32.const 16) "ready"))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-widget (canon lower (func $set-widget) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "widget" (func $widget (param i32 i32 i32 i32 i32)))
    (func (export "init")
      i32.const 0 i32.const 6 i32.const 16 i32.const 5 i32.const 2048 call $widget)
    (func (export "dispatch") (param i32 i32 i32 i32)))
  (core instance $host (export "widget" (func $lower-widget)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "dispatch") (param "channel" string) (param "record-json" (list u8))
    (canon lift (core func $run "dispatch") (memory $memory) (realloc $realloc))))
