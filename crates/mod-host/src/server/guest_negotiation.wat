;; A registry-aware probe: confirmation initializes its UI; every later callback reports what it saw.
(component
  (import "cinnabar:server-experience/ui@1.4.0" (instance $ui
    (type $gui-size (record (field "width" f64) (field "height" f64) (field "scale" f64)))
    (export "gui-size" (type (eq $gui-size)))
    (export "set-widget" (func (param "id" string) (param "text" string) (result (result (error string)))))
    (export "open-screen" (func (param "template" (option string)) (result (result (error string)))))))
  (import "cinnabar:server-experience/messaging@1.4.0" (instance $messaging
    (export "negotiate" (func (param "confirmation-channel" string) (param "request-channel" string)
      (param "schema" u16) (param "fingerprint" string)))))
  (import "cinnabar:server-experience/input@1.4.0" (instance $input
    (export "pressed" (func (param "action" string) (result bool)))))
  (alias export $messaging "negotiate" (func $negotiate))
  (alias export $ui "set-widget" (func $widget))
  (alias export $ui "open-screen" (func $open))
  (alias export $ui "gui-size" (type $gui-size))
  (alias export $input "pressed" (func $pressed))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.tee $old
      local.get 3 i32.add global.set $next local.get $old)
    (data (i32.const 0) "demo.registry")
    (data (i32.const 32) "demo.registry_request")
    (data (i32.const 64) "expected")
    (data (i32.const 80) "init")
    (data (i32.const 96) "shift")
    (data (i32.const 112) "held")
    (data (i32.const 128) "released")
    (data (i32.const 144) "primary")
    (data (i32.const 160) "secondary")
    (data (i32.const 176) "epoch")
    (data (i32.const 192) "ui/terminal.json"))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-negotiate (canon lower (func $negotiate) (memory $memory) (realloc $realloc)))
  (core func $lower-widget (canon lower (func $widget) (memory $memory) (realloc $realloc)))
  (core func $lower-open (canon lower (func $open) (memory $memory) (realloc $realloc)))
  (core func $lower-pressed (canon lower (func $pressed) (memory $memory)))
  (core module $code
    (import "host" "negotiate" (func $negotiate (param i32 i32 i32 i32 i32 i32 i32)))
    (import "host" "widget" (func $widget (param i32 i32 i32 i32 i32)))
    (import "host" "open" (func $open (param i32 i32 i32 i32)))
    (import "host" "pressed" (func $pressed (param i32 i32) (result i32)))
    (import "host" "memory" (memory 1))
    (global $ready (mut i32) (i32.const 0))
    (func $check-ready
      global.get $ready i32.eqz if unreachable end)
    (func (export "init")
      i32.const 0 i32.const 13 i32.const 32 i32.const 21 i32.const 1
      i32.const 64 i32.const 8 call $negotiate)
    (func (export "dispatch") (param i32 i32 i32 i32)
      ;; The registry channel has 13 bytes; tests send only matching fingerprints here.
      local.get 1 i32.const 13 i32.eq
      if
        i32.const 1 global.set $ready
        i32.const 1 i32.const 192 i32.const 16 i32.const 2048 call $open
        i32.const 80 i32.const 4 i32.const 80 i32.const 4 i32.const 2048 call $widget
      else
        call $check-ready
        ;; An empty application record deliberately traps to exercise a fresh instance.
        local.get 3 i32.eqz if unreachable end
        local.get 0 local.get 1 local.get 2 local.get 3 i32.const 2048 call $widget
      end)
    (func (export "action") (param i32 i32 i32 i32)
      call $check-ready
      local.get 0 local.get 1 call $pressed i32.eqz if unreachable end
      local.get 0 local.get 1 i32.const 144 i32.const 7 i32.const 2048 call $widget)
    (func (export "secondary-action") (param i32 i32 i32 i32)
      call $check-ready
      local.get 0 local.get 1 call $pressed i32.eqz if unreachable end
      local.get 0 local.get 1 i32.const 160 i32.const 9 i32.const 2048 call $widget)
    (func (export "epoch")
      call $check-ready
      i32.const 176 i32.const 5 i32.const 176 i32.const 5 i32.const 2048 call $widget)
    (func (export "shift") (param i32)
      call $check-ready
      local.get 0
      if
        i32.const 96 i32.const 5 i32.const 112 i32.const 4 i32.const 2048 call $widget
      else
        i32.const 96 i32.const 5 i32.const 128 i32.const 8 i32.const 2048 call $widget
      end)
    (func (export "resized") (param f64 f64 f64) call $check-ready)
    (func (export "text") (param i32 i32 i32 i32)
      call $check-ready
      local.get 0 local.get 1 local.get 2 local.get 3 i32.const 2048 call $widget))
  (core instance $host
    (export "negotiate" (func $lower-negotiate))
    (export "widget" (func $lower-widget))
    (export "open" (func $lower-open))
    (export "pressed" (func $lower-pressed))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "dispatch") (param "channel" string) (param "record-json" (list u8))
    (canon lift (core func $run "dispatch") (memory $memory) (realloc $realloc)))
  (func (export "action") (param "id" string) (param "collection-index" (option u32))
    (canon lift (core func $run "action") (memory $memory) (realloc $realloc)))
  (func (export "secondary-action") (param "id" string) (param "collection-index" (option u32))
    (canon lift (core func $run "secondary-action") (memory $memory) (realloc $realloc)))
  (func (export "epoch") (canon lift (core func $run "epoch")))
  (func (export "shift-changed") (param "held" bool) (canon lift (core func $run "shift")))
  (func (export "modal-resized") (param "size" $gui-size) (canon lift (core func $run "resized")))
  (func (export "text-changed") (param "control" string) (param "text" string)
    (canon lift (core func $run "text") (memory $memory) (realloc $realloc))))
