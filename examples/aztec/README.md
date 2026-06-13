# Aztec (Williams 1976) - scripted table example

A proof of concept of the table scripting bridge: the original table's
VBScript game logic translated to Lua, plus the static `.table.json` sidecar
(sounds and slingshot animations the engine plays itself, no scripting
needed - see vpinball/vpinball#2263 for the idea).

Install by copying both files next to the table's `.vpx` (file stem must
match):

```
~/vpinball/tables/Aztec (Williams 1976)/
  Aztec High-Tapped (Williams 1976).vpx
  Aztec High-Tapped (Williams 1976).lua
  Aztec High-Tapped (Williams 1976).table.json
```

Keys: `5` coin in, `1` start (press again before ball 2 for more players),
flippers/plunger as usual, `Z` / `/` / `Space` nudge (too many in a row
tilts). High scores and credits persist in a `.store.json` next to the vpx.
