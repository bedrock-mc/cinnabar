(component
  (import "$HUD" (instance $hud
    (export "set-label" (func (param "text" string) (result (result (error string)))))))
  (import "$ENVIRONMENT" (instance $environment
    (export "set-time-override" (func (param "ticks" (option u32)) (result (result (error string)))))))
  (import "$GAMEPLAY" (instance $gameplay
    (type $v (record (field "x" f32) (field "y" f32) (field "z" f32)))
    (export "vector3" (type $vector3 (eq $v)))
    (type $r (record (field "offset" $vector3) (field "roll" f32) (field "fov-delta" f32)))
    (export "camera-rig" (type $rig (eq $r)))
    (export "set-camera-rig" (func (param "rig" (option $rig)) (result (result (error string)))))
    (export "request-command" (func (param "command" string) (result (result (error string)))))))
  (import "$EVENTS" (instance $events
    (type $c (record (field "name" string) (field "values" (list f32))))
    (export "cue" (type $cue (eq $c)))
    (export "emit" (func (param "name" string) (param "values" (list f32))
      (result (result (error string)))))
    (export "poll" (func (result (list $cue))))))
  (alias export $hud "set-label" (func $set-label))
  (alias export $environment "set-time-override" (func $set-time))
  (alias export $gameplay "set-camera-rig" (func $set-rig))
  (alias export $gameplay "request-command" (func $request-command))
  (alias export $events "emit" (func $emit))
  (alias export $events "poll" (func $poll))
  (core module $memory-module
    (memory (export "memory") 1)
    (data (i32.const 0) "$LABEL")
    (data (i32.const 64) "$COMMAND")
    (data (i32.const 128) "probe")
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.get 2 i32.add i32.const 1 i32.sub
      i32.const 0 local.get 2 i32.sub i32.and local.tee $old
      local.get 3 i32.add global.set $next local.get $old))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-label (canon lower (func $set-label) (memory $memory) (realloc $realloc)))
  (core func $lower-time (canon lower (func $set-time) (memory $memory) (realloc $realloc)))
  (core func $lower-rig (canon lower (func $set-rig) (memory $memory) (realloc $realloc)))
  (core func $lower-command (canon lower (func $request-command) (memory $memory) (realloc $realloc)))
  (core func $lower-emit (canon lower (func $emit) (memory $memory) (realloc $realloc)))
  (core func $lower-poll (canon lower (func $poll) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "label" (func $label (param i32 i32 i32)))
    (import "host" "time" (func $time (param i32 i32 i32)))
    (import "host" "rig" (func $rig (param i32 f32 f32 f32 f32 f32 i32)))
    (import "host" "command" (func $command (param i32 i32 i32)))
    (import "host" "emit" (func $emit (param i32 i32 i32 i32 i32)))
    (import "host" "poll" (func $poll (param i32)))
    (import "host" "memory" (memory 1))
    (global $frames (mut i32) (i32.const 0))
    (func (export "init"))
    (func (export "frame")
      global.get $frames i32.const 1 i32.add global.set $frames
      i32.const 0 i32.const $LABEL_LENGTH i32.const 512 call $label
      i32.const 1 i32.const $TICKS i32.const 512 call $time
      i32.const 1 f32.const $RIG_X f32.const 0.5 f32.const 3 f32.const 0 f32.const 0
      i32.const 512 call $rig
      i32.const 64 i32.const $COMMAND_LENGTH i32.const 512 call $command
      i32.const 128 i32.const 5 i32.const 600 i32.const 0 i32.const 512 call $emit
      i32.const 700 call $poll
      global.get $frames i32.const 2 i32.eq
      if i32.const 704 i32.load i32.const $SECOND_FRAME_CUES i32.ne if unreachable end end
      $EXTRA))
  (core instance $host
    (export "label" (func $lower-label))
    (export "time" (func $lower-time))
    (export "rig" (func $lower-rig))
    (export "command" (func $lower-command))
    (export "emit" (func $lower-emit))
    (export "poll" (func $lower-poll))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
