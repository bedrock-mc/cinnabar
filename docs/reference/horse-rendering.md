# Horse model selection

Vanilla selects the client entity with the greatest `min_engine_version` compatible
with the resource pack's engine ceiling. Filename order and `format_version` do not
select the model. An absent or invalid minimum sorts below valid versions.

The pinned pack selects `entity/horse_v3.entity.json`, `geometry.horse.v3`, and
`controller.render.horse.v4`. Its foal uses `geometry.horse.baby` and baby textures.

| Vanilla rule | Observable result |
| --- | --- |
| Neck defaults to 30 degrees around X; tail to 25 degrees | Animation deltas preserve the authored resting pose |
| Saddle, bridle and bits require a saddle | An unequipped horse has no tack |
| Reins require a saddle and rider | An unridden horse has no reins |
| Bags and mule ears are hidden | Ordinary horses retain their own silhouette |
| Rearing approaches one by `(1-s)*0.4+0.05`; release adds `(0.8*s³-s)*0.6-0.05` | Body and limbs ease into and out of the authored rearing pose |
| Horse metadata word bits 5 and 7 select grazing and mouth opening | Horse state does not reuse the generic eating flag |
| Each tick has a 1/200 tail-start chance; the counter advances from 2 through 8 | Tail shaking lasts seven ticks and can restart |

Vanilla compilation keeps only the winning definition before binding geometry,
animations and artwork. All source identities remain in the carrier. Equal winning
minima require pack-order merging; the strict compiler rejects that unsupported
case. The pinned vanilla pack has no such ties. Session-pack version selection and
equal-version merging remain incomplete.

Ordinary actors interpolate completed tick poses. Native render-time variable and
query sampling, the complete horse state product, and sheep-specific grazing
remain incomplete.
