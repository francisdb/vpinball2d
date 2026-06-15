# VPX example table - sidecar

The Visual Pinball [example table](https://github.com/vpinball/vpinball/raw/refs/heads/master/src/assets/exampleTable.vpx)
needs no game logic: the `.table.json` sidecar is pure static config (which
sound a flipper, slingshot, bumper, drain, target, spinner or gate makes, and
which rubbers animate a slingshot - see vpinball/vpinball#2263). No `.lua`.

Install by copying the sidecar next to the table's `.vpx` (file stem must
match):

```
~/vpinball/tables/VPX Example Table/
  exampleTable.vpx
  exampleTable.table.json
```
