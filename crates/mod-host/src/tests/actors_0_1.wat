(component
  (import "$PLAYER_STATE" (instance $player-state
    (type $b (record (field "min" (tuple f32 f32 f32)) (field "max" (tuple f32 f32 f32))))
    (export "bounds" (type $bounds (eq $b)))
    (type $a (record
      (field "session" u64) (field "dimension" s32) (field "runtime-id" u64) (field "player" bool)
      (field "position" (tuple f32 f32 f32)) (field "boxes" (list $bounds)) (field "name" string)
      (field "identifier" string) (field "yaw" f32) (field "pitch" f32) (field "pickable" bool)
      (field "hittable" (option bool))
      (field "look-origin" (option (tuple f32 f32 f32)))
      (field "look-direction" (option (tuple f32 f32 f32)))
      (field "invisible" bool) (field "sneaking" bool) (field "hurt" bool) (field "health" (option f32))))
    (export "actor" (type $actor (eq $a)))
    (export "read-actors" (func (result (result (list $actor) (error string)))))))
  (alias export $player-state "read-actors" (func $read))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.tee $old
      local.get 3 i32.add global.set $next local.get $old))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $read) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "read" (func $read (param i32)))
    (import "host" "memory" (memory 1))
    (func (export "init"))
    (func (export "frame")
      i32.const 256 call $read
      i32.const 256 i32.load8_u if unreachable end
      i32.const 264 i32.load if unreachable end))
  (core instance $host
    (export "read" (func $lower))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
