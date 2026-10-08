# Lifeboat server forms

The game description and action/header buttons were missing, and the selector
placed reserved entries into its game grid, producing extra rows and scrolling.
The repair follows the native factory, predicate and grid contracts.

| Vanilla rule | Engine behavior |
| --- | --- |
| Action title and body enter the factory creation bag | Direct labels and named-source views receive both values |
| Button contents carry ordered roles separately from the count | Role factories receive an array; length remains numeric |
| Completed groups tolerate surplus trailing closes | Visibility bindings survive; invalid interior closes still reject |
| An empty namespace prefix uses the declaring namespace | Relative header references resolve |
| Grid layout consumes evaluated component capacity | Reserved sidebar/header entries do not add game rows |

Regression tests reproduced missing creation text, rejected visibility bindings,
unresolved relative references and 23 cells where only eight should place.
After the fix, all 487 JSON-UI tests pass. The installed server-pack harness also
passes: eight game targets in three columns and three rows, separate sidebar and
header targets, and zero overflow in the game viewport. Missing local fixtures
produce named skip messages; pack files and art remain outside git.

The optimized client at `a4b75c8a17bfed1df748733ccd1f01cbf0996df1` joined Lifeboat
on macOS/Metal, with a 1280×720 logical viewport, 2560×1440 physical output and
DPI 2. Fresh rendered frames show the restored selector sidebar, back/close
icons and eight-card layout without a scrollbar. Clicking Navigator's Mini Game
Selector opened the expected server form. The user then tested on Lifeboat and
accepted the complete result for direct publication to `dev`.

The supplied native screenshots are a near-version visual witness. This closes
the reported server-form compatibility issue; broader typography, rendering and
performance parity gates remain open.
