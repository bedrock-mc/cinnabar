use super::*;
use crate::molang::{Queries, Rng};

#[test]
fn argb_particle_tints_keep_authored_opacity() {
    let mut interner = Interner::with_builtins();
    let mut variables = Vec::new();
    let mut rng = Rng::new(1);
    for (text, expected) in [
        ("#FFFF9200", [1.0, 146.0 / 255.0, 0.0, 1.0]),
        ("#FF9200", [1.0, 146.0 / 255.0, 0.0, 1.0]),
        ("#80FF9200", [1.0, 146.0 / 255.0, 0.0, 128.0 / 255.0]),
    ] {
        let color = parse_color(&Value::String(text.into()), &mut interner).unwrap();
        let actual =
            color.map(|program| program.eval(&mut variables, &mut rng, &Queries::default()));
        assert_eq!(actual, expected, "authored particle tint {text}");
    }
}
