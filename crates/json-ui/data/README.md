# Vanilla JSON-UI vocabulary

`vanilla-ui-vocabulary-1.26.50.txt` contains sorted, unique category/name pairs
from the sample archive pinned by `assets/vanilla-source.json`. It includes UI
references, bindings, buttons, variables, controls, collections (including names
assigned through variables), factory names, renderer names, and English text keys. It contains no control trees or text values.
The source archive is the pinned [Mojang/bedrock-samples](https://github.com/Mojang/bedrock-samples)
release; the archive and its original notices remain local inputs.

Regenerate it with the verified local sample archive:

```sh
cargo run --locked -p asset-compiler --bin assetc -- vocabulary \
  --archive .local/assets/downloads/bedrock-samples-v1.26.50.4-full.zip \
  --source-manifest assets/vanilla-source.json \
  --out crates/assets/data/vanilla-pack-vocabulary-1.26.50.tsv \
  --actors-out crates/protocol/data/retail_actors_1_26_50.txt \
  --ui-out crates/json-ui/data/vanilla-ui-vocabulary-1.26.50.txt
```

The `json_ui::vanilla` tests check every declared name in its own category.
Product extensions and protocol JSON fields stay outside this contract.
