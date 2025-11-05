#!/usr/bin/env bash
# download the example table

curl https://github.com/vpinball/vpinball/raw/refs/heads/master/src/assets/exampleTable.vpx -L --output ./assets/exampleTable.vpx
echo "Downloaded exampleTable.vpx to ./assets/"
