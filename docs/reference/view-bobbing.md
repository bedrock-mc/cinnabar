# View-bobbing preference and the hand rig

The vanilla hand renderer assigns `variable.bob_animation`.
Cinnabar's authored player animations consume that Molang variable separately
from the camera's bob transform. Disabling only the camera transform leaves the
hand's authored movement running.

The local-player feed therefore carries the camera settings authority's
view-bobbing preference into the actor animation context. Remote actors keep
the default context. The engine variable is set before pack scripts execute;
attachables inherit the evaluated owner's variables through their existing
inheritance path. Attack and equip drivers remain independent of the bob gate.

The regression fixture compiles an independently authored hand animation and
toggles the preference during movement, then verifies an attack still animates
with bobbing disabled. No native source or artwork is copied into the fixture.
