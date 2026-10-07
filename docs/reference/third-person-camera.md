# Third-person camera position and look

Perspective changes refresh the local rig's draw context on the current frame,
including frames without an actor tick. First-person and world-space bone poses
must not interpolate into each other. This refresh keeps motion, status, actor
ticks and clip time unchanged, and invalidates cached bone conversions. Tests
cover both directions and switches on/between ticks; Windows acceptance is pending.

## Shared render position

The vanilla camera position starts from the actor's interpolated riding
position at the render fraction and applies the interpolated eye/stance
offsets. The actor's world transform starts from the same interpolated riding
position at the same render fraction.

Cinnabar's camera already samples the local physics render position. A local
actor fed the latest completed physics state into the remote actor clock can
produce a different sample, especially when a frame completes multiple physics
ticks or when the two clocks have different baselines. The local rig therefore
uses the physics render feet for its world translation before equipment, cape,
skin layers, lighting and selection are built. Its body rotation, model scale
and bone animation remain those of the driven rig. Remote actors keep their
network interpolation.

View bobbing independently interpolates walk
distance and bob amplitude. It does not introduce another third-person actor
position interpolation.

## Front-view look coordinates

Camera look input is polar/elevation then azimuth, in that order. Its `invert_x_input` flips the polar component;
it is not a mouse horizontal/player-yaw inversion. Direct look
integrates pitch from the first component and yaw from the
second. The reverse camera's player update negates its forward
vector before converting back to player look, while reverse orbit setup
initializes from the opposite look vector.

Cinnabar retains player-space look, so perspective selection must not reverse
the player's horizontal mouse input. Front/rear view placement belongs in the
camera transform, separate from the actor's look and movement direction.

Reverse orbit setup constructs its spherical offset from the
full player look vector, including elevation. The front camera therefore keeps
pitch when moving to the opposite side of the subject. The look-at update uses global Y as its
up vector; using the player's pitched up vector would introduce roll. Tests
cover both turn directions through the complete F5 cycle and front-camera
pitch/yaw combinations up to the existing player pitch limit. This identifies
the setup and coordinate-space contracts; it does not close the broader camera
preset-limit or sensitivity measurement gates.
