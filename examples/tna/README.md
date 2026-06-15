# Total Nuclear Annihilation (Spooky 2017) VPW - sidecar

This table needs no game logic in the engine: the `.table.json` sidecar is
pure static config (flipper / slingshot / bumper / drain sounds and the five
slingshot rubber animations - see vpinball/vpinball#2263). Each event picks a
random sound from its list, matching the original script. No `.lua`.

Install by copying the sidecar next to the table's `.vpx` (file stem must
match):

```
~/vpinball/tables/Total Nuclear Annihilation (Spooky 2017)/
  Total Nuclear Annihilation (Spooky 2017) VPW v2.3.vpx
  Total Nuclear Annihilation (Spooky 2017) VPW v2.3.table.json
```
